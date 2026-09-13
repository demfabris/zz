# TUI-012 attempt-01: the superset commands beside tmux behaviour

Cycle 7, superset lane, on the campaign box (alienware, CachyOS, Linux). Every run below is a
real run of the file it names, at the revision `environment.txt` records, against the pinned tmux
`d77c9dc6` in `compat/.cache/tmux-src/tmux` of the shared checkout.

## What landed

`compat/tui-superset.sh`, new. Nothing else in the tree changed except the ledger, the three gap
reasons this batch owns, and the two generated reports. No crate, no wire field, no option name
and no zz-only string, prompt, label or format was removed or changed, so no corpus scenario
depends on this landing; `rg` over `compat/scenarios` for every message the fixture asserts finds
none of them, and the only row that names a superset verb at all,
`compat/scenarios/command-item-format.txt`, names `select-pane-kind` in a comment. That row was
run anyway and passes: `corpus-command-item-format.txt`.

## Files here

- `environment.txt` - box, revision, binaries, pin, locale, and the environment every run used.
- `tui-superset.self-check.txt` - `compat/tui-superset.sh --self-check`, exit 0.
- `tui-superset.run-1.txt`, `run-2.txt`, `run-3.txt` - three consecutive runs, each exit 0,
  189 asserted cases, 0 recorded.
- `tui-screen-diff.txt`, `tui-screen-diff.self-check.txt` - the sidebar cases this lane owns,
  unchanged and still green, exit 0.
- `tui-pane-geometry.txt`, `status-row.C-locale.txt`, `attached-client.txt` - the surfaces every
  lane runs, exit 0. `status-row.sh` is run under `LC_ALL=C LC_TIME=C`, its control on this box.
- `cargo-test-zz-mux.txt`, `cargo-test-zz.txt`, `clippy-zz-mux.txt` - the crate checks. No crate
  source changed; `compat/tmux-gaps.json` is read by `zz-mux`'s manifest tests and by a `zz-daemon`
  test, so those are the ones that could notice the gap-reason edit.
- `corpus-command-item-format.txt` - `compat/run.sh command-item-format`, the only corpus row that
  names a superset verb at all, exit 0.
- `tracker-checks.txt` - both trackers, valid and current.

## The three clauses

**Clause 1, the verbs.** `NATIVE_COMMAND_NAMES` in `crates/zz-protocol/src/catalog.rs` is the
list, and the fixture walks it out of the file itself rather than out of a copy: 25 names, every
one reachable from a raw TUI by that name, which is the opposite of `unknown command: NAME`. Each
verb a raw TUI can run is then driven twice, once from the CLI and once through an ordinary
`bind-key -n` binding, and each case asserts either what it draws or the exact message it refuses
with. A refusal is a declared case, not a divergence: the pin has no `agent-send` to compare with.

The binding half is a pass of its own, `bindings`, and it covers all twenty-five names: one key is
rebound before each case so no case inherits the one before it, and the answer is read from the
client's message row, which is where a command's answer lands when the command came from a key.
`reload-config` says `Reloaded zz configuration` there, the sixteen argument-validation and
target-resolution refusals say their exact message, `send-text` is read from the pane's own grid,
`focus-sidebar`, `tools`, `split-picker`, `split-browser` and `split-agent` are read from the
screen, `new-browser` from the window count, and `debug-marker`, which answers nothing anywhere, is
bound as a sequence whose first command reports on the row.

The refusals asserted whole, message for message: `focus-sidebar requires an interactive client`
(the CLI is not an interactive client, the way the pin refuses a client command with no client),
`browser screenshots require the zz app`, `agent commands require the zz app`, `editor panes are
experimental; enable experimental-editor-pane in Settings > Advanced first`, `select-pane-kind
requires exactly one of: terminal, browser, agent, editor`, `pane %N is not awaiting a type
selection`, `pane %N is not an editor`, `pane %N is not an agent`, `invalid target: %N is not an
agent pane`, `no tmux configuration found` (HOME and XDG_CONFIG_HOME are inside the scratch
directory, so there really is nothing to import), `terminal search is unsupported here`, the two
`has no shell-integration marks` messages, and the seven argument-validation messages.

What each surface draws, asserted: the sidebar tree; the picker card with all four choices and its
key line; the browser card with its URL following `set-browser-url`, `set-browser-tabs` and a
bound binding; the Agent card with its provider label following `set-agent-provider`; and the
command-output overlay opened by `tools`.

**Clause 2, the terminal around them.** For the sidebar, the picker, a browser pane, an Agent pane
and the command-output overlay: create (`new-window`), select (`select-window`, `select-pane`),
split, resize, detach and reattach run on both sides against explicit targets, the pin's own pane
and window inventory is compared while the surface is up, and once the surface is gone the whole
decoded screen and the whole cursor tuple are the pin's. A pane surface is removed rather than
withdrawn, so the pin gets the same pane arithmetic - an ordinary split where zz gets the zz kind,
and a kill on both sides afterwards.

Detach and reattach separate the two kinds of state and both halves are asserted: a pane surface is
session state and is still drawn for the client that comes back, with the layout the pin has; the
sidebar and the command-output overlay are the client's and are gone, with the canvas the pin
draws.

Input ownership is asserted where it lives: a key in the sidebar's own table never reaches the
pane while the sidebar is focused, and the pane owns the keyboard again the moment the sidebar is
unfocused or withdrawn. The same for the overlay: a key pressed while it is up never reaches the
pane. Both are ordered without a sleep, because both keys travel the same client input stream and
the withdrawal the second key asks for is on the screen before the first key can still be
in flight.

**Clause 3, a second client.** Two clients on each side. Showing the sidebar in the first leaves
the second client's whole screen the pin's, before, during and after; the first client's screen
carries the tree and the second's does not, so the identity half is not passing on a sidebar that
never drew. A browser pane, unlike the sidebar, is session state: both clients draw the card, and
neither ever draws a frame, because no Kitty graphics reach a pane inside the pinned tmux and the
screenshot verb says so in the message a raw TUI gives.

## Measurements worth keeping

- **The sidebar takes columns from the session.** `focus-sidebar` makes the client report
  `120 - 28 - 1 = 91` usable columns, so the shared window becomes 91 wide for every client on that
  session, exactly as an ordinary 91-column tmux client would make it. Withdrawing it restores 120.
- **A client wider than its window is a general canvas gap, not a superset one.** Measured
  2026-09-13 with two PLAIN clients, 120 and 91 columns, and no zz verb anywhere: the pin's
  120-column client draws a border at column 91 and middle dots across 92..119 on every window row,
  and the raw TUI draws spaces. It reproduces with no superset command in sight, so it is the
  ordinary multi-client canvas and belongs to the client-inventory obligation, not to this one.
  Clause 3 therefore sizes every client to the window: every cell of both second clients is
  asserted instead of a band of columns being waived to dodge it.
- **Unfocusing the sidebar changes no cell.** Escape (SidebarCancel) leaves the tree drawn and
  identical, decoded screen for decoded screen, while the keyboard goes back to the pane. The only
  observable for that state is the next key reaching the pane, and the first character after an
  Escape merges with it into one Alt- key at the terminal parser, so the fixture sends the Escape
  and the key in separate bounded rounds and passes only when the pane's own grid holds the key.
- **The command-output overlay withdraws on the first layout change.** `rename-window`,
  `set-option` and `select-pane` leave it up; `split-window` takes it down by itself. Nothing is
  pressed to close it in that case, and that matters: with the overlay already gone, an Escape
  sent to close it reaches the pane instead, and the pane's terminal eats the next typed line with
  it. An earlier draft of this fixture did exactly that and spent three runs looking like a zz bug.
- **The clear and the marker are one command line.** Sent as two, the second line's echo can
  interleave with the first's before the shell has read it - measured here as
  `pprintf 'MARK-%s\n' closedrintf '\033[2J...'` in the pane's own grid, with neither command run.
- **A bound command sequence does not stop at a failure, on either binary.** Measured 2026-09-13
  on both: a key bound to `<failing command> ; rename-window TOKEN` renames the window on the pin
  and on zz alike, so a later command in a bound sequence proves that the binding fired, never that
  an earlier command succeeded. A zz client message otherwise stays on the row indefinitely - still
  there after eleven seconds - but a status repaint that follows it overwrites it, which is why the
  binding pass reads a verb's outcome from the message it prints and not from a token after it.
- **The client's message row is cut at the client's width.** The two shell-integration messages are
  longer than 120 columns, and their cases assert the cut text rather than a prefix they chose.
- **The picker's card is drawn for the active pane only** (`render.rs` draws the picker arm under
  `if active`), where a browser or Agent card draws either way. Clause 2 selects the picker pane
  again after the ordinary commands before asking for its marker.
- **The picker's Editor choice is refused behind a product setting**, not behind a parity profile:
  the other three kinds need no flag, and the refusal is drawn on the client's message row while
  the card stays up.

## Gaps

`commands.native-superset`, `keys.native-defaults` and `pane.floating-model` all stay accepted and
this landing closes no item in any of them. For all three the reason is the same and it is worth
stating rather than leaving as silence: the pin has no counterpart for a zz-only name, a zz-only
default key table or a zz-only floating surface, so there is no pin behaviour for the raw TUI to
start honouring. Each of the three carries a dated 2026-09-13 measurement saying so, and
`commands.native-superset` also had its stale count corrected: the array holds 25 names, not the
23 its reason was written against.

## Self-check

`--self-check` drives one deliberate fault per channel and requires the same code the real cases
use to report it: a name the server does not know, a refusal message that does not match, a verb
that accepts where a refusal is declared, a surface that never drew, a message row that never
carried the message, a cut message the row never carried, a bound verb whose key was never
pressed, a key the pane really did take, a canvas that is not the pin's, and a layout only one
side has. Ten faults, ten reports.
