---
type: Playbook
title: Running the tmux compatibility harness
description: How to run the pinned tmux differential corpus, read topology, geometry, format, and query-stdout results, and record known divergences.
resource: compat/run.sh
tags: [tmux, compatibility, differential-testing, geometry, playbook]
timestamp: 2026-08-26T00:00:00-03:00
last_updated: 2026-08-28
last_updated_by: Codex
---

# Overview

The harness feeds each scenario command to zz and tmux at commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`. After each command, it queries both servers
with matching explicit `list-sessions`, `list-windows`, and `list-panes` formats. The
runner compares command exit classes and topology as strict results. It also compares `fmt:` format
queries and generic `out:` command stdout as separate byte-exact strict channels. Geometry
differences fail under `--strict-geometry`, which is how CI runs the harness.

`compat/run.sh` builds `target/debug/zz` with your normal environment before the scenario
runner creates its scratch `HOME` and `XDG_CONFIG_HOME`. The tmux fetcher clones and builds
the pin under `compat/.cache/`. The canonical oracle check accepts `tmux next-3.8` only when the
binary lives at the root of a clean source checkout at the exact pin and its companion build stamp
matches the commit, version, fetch recipe, and binary checksum. `ZZ_COMPAT_TMUX` can select another
cache built by the same fetcher, but a version-matching prebuilt or an unstamped clean checkout
cannot satisfy the oracle.

The cache stays valid while its source HEAD is clean at the pin and its build stamp matches the pin,
version, fetch script, and binary. A mismatch rebuilds tmux before the gate runs; a dirty checkout
is refused rather than attested.

# Tracker and generated report

`compat/tmux-gaps.json` is the sole live TODO and status source. Schema 3 assigns stable IDs to
active gaps, stores the manifest date in `updated_on`, and keeps completed work in `closed`. The
generated [tmux compatibility gap report](/tmux/gaps.md) is the readable view. Do not maintain
counts or open-item rosters in the philosophy, roadmap, divergence matrix, or research snapshots.

Run the fast gate before choosing or landing a compatibility slice:

```sh
just compat-check
```

The recipe calls `compat/check.sh`, which fetches the pinned tmux binary once, validates the oracle
and registry, asserts that all four named manifest tests still exist, then runs the full `zz-mux`
library suite. Linux CI runs the same command after restoring the pinned tmux cache. A full
`compat/run.sh` checks the oracle and tracker before executing scenarios.

Oracle schema 4 records 92 commands, 78 aliases, and 572 accepted command-flag shapes: 318
valueless, 246 required-value, and 8 optional-value. Every command also carries its positional
minimum and maximum. The source pass also records 14 commands that use nine custom `args_parse`
callbacks as six effective rules. The remaining inventories contain 180 options, 198 global
format-table names, 14 source-enumerated names across the selected `command-item`, `list-commands`,
and `list-keys` contexts, 68 hooks, and 303 default bindings across `root`, `prefix`, `copy-mode`,
`copy-mode-vi`, and `move`.

The Rust gate reconciles command and alias names, flag arities, positional bounds, custom argument
rules, option names, global and selected context-format names, and hook names. It also classifies
native commands, native aliases, zz-only flags on tmux command names, and every zz-only default key.
It derives the guarded native-name roster from the catalog minus the pinned oracle and checks every
pinned canonical prefix against the resolver. It pairs every constant-backed format with a manifest
item and tracks every missing default key across all five tmux tables. For each shared default key,
it also reconciles the rendered command and repeat bit or requires a named `binding:` divergence. The
three selected context rosters contain 1 `command-item` name, 3 `list-commands` names, and 10
`list-keys` names. zz implements all 14. `formats.command-item-context` closed on 2026-08-24: the
mux dispatch chokepoint carries the canonical entry name into every command it runs, so `#{command}`
expands inside any command item and stays empty outside one. The daemon-preempted half closed under
`formats.daemon-command-item-context`; its immediate format hooks now carry the same canonical name,
and the daemon's post-spawn `new-window`/`split-window -P -F` pass retains it while adding live pane
facts. Delayed subscriptions and prompts stay outside an item.

`formats.command-argument-expansion` closed five target-sensitive paths on 2026-08-24. The current
`command-item-format` scenario covers the positional names for `rename-session` and
`rename-window`, optional option names for both show commands, `select-pane -T`, both
`new-session` names, formatted `new-window -n`, literal `break-pane -n`, and shared name cleaning.
Its fixtures use exact non-current targets and cover Unicode, backslash identity, clean-name reuse,
literal format tokens, and the pin's pane format type for both rename commands. Control-byte
rejection and expansion-count assertions stay in focused
Rust tests because this line-oriented fixture cannot carry those values. The
focused run prints the authoritative step count; this playbook does not duplicate that moving
number. `formats.new-session-name-expansion` closed `new-session -s` on 2026-08-25, and
`formats.name-validation-cleaning` then closed the shared `new-session`, `new-window`, rename, and
literal `break-pane` name pipeline. `formats.creation-name-edges` closed the pin's second
`new-window -S` lookup expansion and `break-pane -n` automatic-rename side effect on 2026-08-25.
`formats.buffer-path-expansion` closed both buffer paths the same day; the focused
`buffer-path-format` scenario covers one-pass expansion, format-before-home ordering, canonical,
alias, unique-prefix, and user-alias command identity, and load/save file effects.
`native-prefix-isolation` covers the 25 unique tmux prefixes that native names had changed, plus
exact alias and user `command-alias` precedence. Matched empty, multi-command, and unparsable alias
shadows are unit-tested at the mux and daemon dispatch seams instead: their expected zz result is a
loud `unknown command: <typed name>`, so they are not a differential claim while
`aliases.command-bodies` remains open.
Protocol v74 closes Control's former static unknown-name precheck through focused daemon and CLI
tests. The client prepares the entire initial argv unit or complete LF line before opening execution
frames, observes one daemon alias snapshot for that unit, preserves command numbering and
notifications, and executes the prepared invocation with ordinary read-only authorization. These
tests do not claim tmux-compatible empty or multi-command alias bodies. The strict
`smoke/control-alias-prepare` fixture adds pinned proof for one whole-line alias snapshot and a
whole-line preparation error that aborts before either surrounding effect. The strict
`smoke/cli-chain-parse-abort` fixture proves that a local CLI unknown-name parse failure aborts
before an earlier mutation while a runtime command failure keeps the earlier effect and prunes the
later command. This atomicity requires a live compatible daemon. When preparation fails open before
autospawn, an earlier starting command may take effect before a later unknown name. Only the
unknown-name error shape is pinned here; malformed alias-body text remains zz-defined while
`aliases.command-bodies` is open.
Local attach, stdin, kill, and malformed-alias preprocessing also has focused binary coverage.
Remote `--host` preparation and whole-vector flag or arity prevalidation remain explicit tracker
gaps, as does config or source-file replay-group abort. Per-command flag and arity diagnostics at
dispatch now match the pin.

Oracle schema 4 closes callback discovery, not callback behavior. The typed Rust sidecar mirrors the
12 implemented callback commands, and `COMMAND_ARGS_PARSE_BEHAVES` stays empty until runtime tests
prove a command's rule. The manifest therefore carries 12 `args-parse:` items. The unimplemented
`choose-client` and `switch-mode` callbacks need no second item because their `command:` items cover
the whole command.

Six semantic gaps remain: runtime adoption of the inventoried argument rules, open-ended or dynamic
context-format names, nonconstant format behavior, hook production, runtime behavior for shared
bindings, and consumer truth for names in option `BEHAVES`. `tracker.semantic-coverage` owns that
work. Shared command-flag diagnostics closed on 2026-08-28 without retaining the earlier partial
daemon roster. The catalog parser covers 83 implemented upstream canonical commands and 74 aliases
through mux execution, daemon preflight, and stored commands. Exact native attach shares the
leading-option diagnostics while keeping its positional-session boundary and extensions. The
`smoke/command-flag-errors` fixture byte-compares 516 probes on each server: 513 failures covering
unknown and invalid flags, help usage, missing required values, and unsupported-before-unknown
ordering, plus three successes proving required-value absorption. It checks pane, buffer, file,
binding, and hook sentinels. Differential scenarios, attached-client fixtures, unit tests, and
manual GUI checks remain the behavioral evidence.

Regenerate the readable report after changing the manifest:

```sh
python3 compat/tmux-tracker.py write-report
python3 compat/tmux-tracker.py check
```

Use the registry vocabulary consistently:

- `decision` is `adopt` for tmux behavior zz will implement, `native` for a zz presentation or
  ownership choice, `park` for work without current product demand, or `never` for a permanent
  exclusion.
- `status` records product disposition as `open`, `blocked`, or `accepted`. It does not describe
  dependency readiness.
- `depends_on` records delivery order between active gaps. An open gap may depend on another gap,
  while a blocked gap may have no tracked dependency.
- `priority` is `now`, `next`, `later`, or `none`; `ease` is `easy`, `medium`, `hard`, `hardest`, or
  `none`. Accepted items use `none` for both.
- `items` holds normalized upstream, arity, positional-bound, `args-parse`, selected context-format,
  native-extension, semantic, presentation, and protocol identifiers. The source gate reconciles
  structural identifiers where code exposes an inventory. `evidence` points to source, tests, or
  scenarios; `acceptance` states the condition that closes or accepts the gap.
- `updated_on` changes with the manifest. Completed adopt work moves from `gaps` to `closed` with
  the same ID, a closure date, evidence, and a short resolution.

## Coverage freshness

`compat/results/summary.md` is the persisted canonical artifact. The 2026-08-28 checkpoint contains
88 scenarios and 1,487 steps against pinned tmux `d77c9dc6`. Every ordinary row is clean.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The attached-client fixture is `PASS`. The expanded
corpus pins capture routing and ranges, manual window geometry,
join and break placement, pane-local and creation-time environments, empty panes, post-split zoom,
last-pane input gating, buffer rename, source path formatting, and the small accepted-flag cluster.
`list-keys-padding` contributes 46 byte-exact checks for default padding, note selectors, ordering,
positional filtering, `-1` aggregates, stock repeat metadata, and canonical Space spellings;
`smoke/cheap-flags` contributes 22 checks for `new-window -b` and `unbind-key -a/-q`; and
`smoke/kill-filters` contributes 17 contextual `kill-session`/`kill-window`/`kill-pane -a -f`
checks. `smoke/source-file-depth` contributes 4 command-client checks for the 50-invocation source
limit and the refused 51st. `smoke/source-file-diagnostics` contributes 12 checks for parser and
path diagnostics plus replayed runtime failures, continuation, and outer propagation. Its final
check sources the active default config, a loud missing middle path, an after file, and the default
again. It pins rc 1, declared `-v` order, later-file continuation, and final `DAD` state.
`source-file-format` contributes 40 checks for parse-only, target, target-format, quiet miss,
verbose order, and final state. `smoke/source-file-control` contributes 12 focused checks including
Control verbose suppression, replayed runtime error delivery, the three-level root-miss,
middle-miss, leaf-output guard order, the full return-status matrix, queued Return precedence,
immediate hook flags-0 frames, background inserted-command frames, and parser flags-1 plus hook
flags-0 read-error placement and hidden numbering. The source-read check covers multiple matched
read failures before replay, one completion after descendants, raw unframed diagnostics, retained
status, and later-line continuation. Its status coverage includes actual self-detach, nonself and
no-victim detach, alias targeting, sticky background failures, and `%end` before `%exit`; a manual
`detach-client -a` probe also matches the pin. Protocol v81 extends the same 12-check row with direct
and sourced foreground `run-shell` output after an empty flags-1 guard, exact-recipient raw delivery,
same-line continuation, and unchanged Control retval. The row also requires resolved `-t` and
ordinary `run-shell -b` output to stay off raw Control. It excludes pane-view notifications because
tmux enters a shared pane view while zz opens its native per-Interactive command-output view and
emits no `%pane-mode-changed`. The pinned foreground-disconnect server crash is an intentional
non-parity and stays outside the scenario. `resize-directions`
contributes 16 checks for bare direction flags with
the default amount 1, attached amounts such as `-L2`, separated amounts such as `-L 2`, and the
existing absolute resize forms. `formats-values` also proves explicit
startup `config_files`; both
servers start with `-f /dev/null` so that fact is symmetric. `native-prefix-isolation` contributes
29 steps: 28 byte-exact command-name queries plus one alias setup, without plugin-corpus
dependencies.
`smoke/daemon-invalid-flags` contributes three checks: it first removes any inherited sentinel,
then proves representative daemon-dispatched flags reject before callbacks or buffer mutation, and
finally requires the fixture to publish its clean marker.
`smoke/positional-maximums` contributes three checks: it clears inherited state, then requires exact
canonical maximum errors for all 71 generic-CLI-routed finite commands and 62 aliases,
and finally requires unchanged pane, buffer, and file state. The exhaustive daemon test also covers
the exact attach engine path, which the native CLI intentionally extends with a positional session.
`smoke/positional-minimums` contributes three checks: it clears inherited state, then requires exact
canonical minimum errors for all fourteen commands and aliases before missing-target resolution,
and finally requires unchanged pane, buffer, and file state. Focused daemon tests separately prove
that rejected commands do not change menu, confirmation, or wait state.

The checked-in summary includes the current focused counts: `smoke/source-file-diagnostics`,
`source-file-format`, and `smoke/source-file-control` contain 12, 40, and 12 steps, and
`resize-directions` contains 16. The summary SHA-256 is
`6b7a0261956e84d7340c9ef34f4de0962964215b3cc8eb055a79236acdc257c6`.

`compat/run.sh --check-summary` compares the exact current scenario paths, static step counts, and
all seven stored row cells against the ordinary clean tuple or each registered known tuple. It also
requires its persisted attached-client status to be `PASS`. The check passes for the 2026-08-28
canonical checkpoint and exits before building or running either server. Linux CI first asserts that
`compat/results/summary.md` is tracked, then runs
the inventory and result check after checkout. A named partial run, a headless-only full run, a failed
run, or a run with a SKIP cannot overwrite the canonical report. After a complete strict run with
`--attached-client`, CI diffs the full tracked summary, so changes to Steps, TOPO, GEO, FMT, OUT,
WARN, or the attached proof fail the job. Per-scenario logs remain ignored scratch data, while the
canonical summary stays versionable.

`smoke/config-grammar` intentionally expects the invalid-octal `%config-error` from tmux only. The
nested zz control client still does not publish that diagnostic; the state readbacks separately
prove that both parsers abort the file at the same point.

# Running the corpus

Run the strict corpus and attached-client contract from the repository root:

```sh
just compat --strict-geometry --attached-client
```

`compat/run.sh` without flags remains the non-strict headless-only form. It prints the temporary
report but leaves the canonical combined summary unchanged.

Pass scenario names to run a subset. Names may include or omit `.txt`.

```sh
compat/run.sh windows panes
compat/run.sh known/known-geometry-gap.txt
```

Check whether the persisted inventory and attached proof are current:

```sh
compat/run.sh --check-summary
```

## Startup diagnostic differential

`compat/startup-diagnostics.sh` is a separate seven-case gate for clientless startup causes. Run it
after building the debug binary and fetching the pinned oracle:

```sh
cargo build -p zz --bin zz
compat/startup-diagnostics.sh target/debug/zz compat/.cache/tmux-src/tmux
```

The script requires all seven cases: initial Control cold start; detached launch followed by late
Control attach; startup list-output discard; explicit-root failure ordering; multiline cause
prefixing and completion-line location; daemon-restart redelivery; and Interactive delivery with a
global drain. It compares normalized Control transcripts, checks detached streams and status, and
drives the attached Interactive view through real outer PTYs.

The oracle must be the checkout-root `tmux` executable from a clean checkout at exact commit
`d77c9dc6aa021e4bc61f0da128c591af695e6466`, report `tmux next-3.8`, and match the build stamp's
commit, version, fetch-script checksum, and binary checksum. The probe requires GNU `timeout`, wraps
commands in real 15-second deadlines, uses 500 ms bounded polls, and stops readiness loops after 10
seconds. A missing case or any skip fails the run.

The final debug run passes all seven cases with no skips. This focused script does not call
`compat/run.sh` or regenerate the current `compat/results/summary.md`.

Run the real attached-client fixture separately after building zz and fetching the pin when
debugging it in isolation:

```sh
compat/attached-client.sh target/debug/zz compat/.cache/tmux-src/tmux
```

Pinned tmux owns two isolated outer PTYs and drives an inner zz attach beside an inner tmux attach.
The fixture polls semantic state rather than comparing native presentation. It covers readiness,
root/prefix/prefix2 bindings, copy-mode entry/exit, prompt-driven window rename, choose-tree row
keys, choose-buffer paste/deletion, exact nested-attach refusal, and the attached status message
for a refused 51st `source-file` invocation. It also checks that `list-keys -1` shows a timed status
without replacing the terminal with command output; the short result marker comes from the binding
note and does not appear in the typed prompt. The local Control probe runs `-C` from each outer PTY,
requires existing-session refusal for `attach-session` and `new-session -A`, permits a fresh `-A`
miss, and pipes stdin through a final attach to prove a nonterminal stdin does not publish tty
identity. The daemon unit matrix covers `new-session -Ad`; the attached fixture does not. The
command-output probe builds a 96-line local transcript and runs on both sides. It checks line and
page movement, vi Escape selection clearing without exit, search cancel, search editing and submit,
`n`/`N`, selection-to-paste-buffer, a live custom `copy-mode-vi` binding, a live switch to the emacs
table, vi `q` cancel, and emacs Escape cancel. It verifies the created paste buffer contains the
selected match and then removes it. The current full fixture passed for zz and pinned tmux after
independent review of the fresh-session marker.

The alert-lifecycle probe uses fresh non-current monitored windows. It replaces a 1,500 ms sticky
message with a 5,000 ms Bell alert, writes new terminal output behind it, and proves the current
screen stays frozen for 1.8 seconds across the old deadline. One elapsed endpoint capture requires
the alert to remain visible while the terminal marker remains hidden, so capture-pane process cost
cannot stretch a poll-count clock past the alert expiry. F12 plus Enter then proves one key
dismisses the alert, reaches the pane, and releases the latest viewport well before the alert's own
expiry. The alert window remains unvisited with `#{window_bell_flag}` equal to 1. The probe rings
that same pane again, sees a second Bell message, and repeats the 1.8-second freeze and dismissal
proof while the flag remains set. It then waits 5.2 seconds for the pin's stale positive timer to
drain, changes `display-time` to zero, and repeats the hidden-output and input-release check on
another fresh window. Match the stable `Bell in window` prefix: at 80 columns, the TUI status
surface can truncate the trailing index beside its detach hint. The probe covers ordinary
incremental TTY freeze. A forced structural redraw may expose the latest parsed state.

This focused proof does not cover command-output
mouse behavior, OS clipboard delivery, ordinary TUI pane copy-search editing, SSH transport, pixel
comparison, or the 29 unsupported window-copy actions. It does not update the canonical summary on
its own. The 2026-08-26 strict-plus-attached run persisted this fixture as `PASS`.
Failure output includes both
outer screens and zz daemon stderr; cleanup removes outer servers before inner servers.
`--attached-client` runs it after the headless scenarios and includes it in the overall exit status
without adding a fake row or step count to the canonical summary. Its `PASS` status is persisted
below the scenario rows. A fixture failure or an omitted fixture prevents that full run from
replacing the prior combined summary.

Geometry differences do not change the default exit status. Use strict mode when you want
them to fail the run:

```sh
compat/run.sh --strict-geometry
```

Strict mode is the CI contract: the Linux workflow leg runs `compat/run.sh
--strict-geometry --attached-client`, so every scenario outside `known/` must stay TOPO-clean and GEO-clean
against the pin. Since the cell-authoritative layout landed, a headless zz window is born
at tmux's 80x24 and every layout operation runs the pin's integer arithmetic, which is what
makes exact-geometry diffing possible.

FMT and OUT differences fail in both modes. `--strict-geometry` changes only GEO handling.

Smoke scenarios under `compat/scenarios/smoke/` are part of the default corpus. Each declares
`corpus: none` or `corpus: required`; placement controls smoke-mode byte-exact stdout/stderr checks,
while this metadata alone controls plugin acquisition and offline eligibility. When the pinned
plugin cache is absent or cannot be fetched, the run executes corpus-independent smoke scenarios
and prints a visible SKIP for each plugin-dependent scenario. A skipped smoke is never reported as
a pass. Any SKIP makes the run exit nonzero, discards its temporary report, and leaves the last
complete canonical summary unchanged.

# Reading results

The combined full runner writes `compat/results/summary.md` only after the attached-client fixture
passes. Each row gives the number of executed steps, TOPO, FMT, OUT, and WARN status, plus the number
of steps that produced a GEO difference. The final section preserves the attached-client `PASS`.

Open `compat/results/<scenario>.log` for the command status and per-step unified diffs:

- `COMMAND EXIT-CLASS` compares success with failure. Matching nonzero exits pass because
  both servers refused the command.
- `TOPO` compares session/window counts, names, active indexes, and pane indexes. Any
  difference fails a normal scenario.
- `GEO` compares window and pane cell dimensions plus each window's complete raw
  `#{window_layout}` string, including its checksum and leaf pane ids. Zero-based boot allocation
  now aligns the two sides, so this catches pane assignment permutations as well as structure and
  geometry. The runner reports these differences by default and fails them under
  `--strict-geometry`.
- `FMT` compares stdout from a shared `fmt:` line byte for byte. Both `display-message -p`
  invocations must exit zero. A matching error still fails the FMT step.
- `OUT` compares stdout from any shared query command prefixed with `out:` byte for byte. Both
  commands must exit zero; matching failures still fail the OUT step.
- `WARN` is the smoke-only config channel. It checks each side's expected `%config-error` lines
  and independently checks whether the `source-file` control block ended with `%end` or `%error`.
  The pin does not emit `%config-error` for every execution-time config failure, so both signals
  are required.

The log captures each step's stdout and stderr. In normal scenarios the runner ignores stdout for
ordinary command lines; `fmt:` and `out:` lines enter their respective comparisons. Smoke scenarios
also compare ordinary command stdout byte for byte.

The runner starts zz on a short `/tmp/zzc-<pid>.sock` path and starts tmux with
`-L zzc-<pid> -f /dev/null`. Its exit trap stops both servers and removes both socket files.

The headless scenario rows do not prove copy mode, choose-tree, choose-buffer, command-prompt,
default prefix behavior, packaged launcher attach, or native GUI rendering. The combined strict run
adds the attached-client proof. On macOS, build and exercise the real app launcher separately:

```sh
just build mac
compat/packaged-cli.sh dist/zz/zz.app
```

That fixture verifies CEF resources and the bundle signature, clones the whole app under a path
containing spaces, and drives bare/new/attach against empty and existing daemons. Its PTY cases also
pin detached `new-session -x`/`-y` geometry, attached client dimensions, read-only input rejection
and output visibility, native detach, and `attach -d` peer eviction. The detach paths require exit
status zero plus `[detached (from session NAME)]`; the read-only path processes a later copy-mode
transition before checking that earlier typed input never reached the pane, avoiding a sleep-based
negative assertion. It does not install or notarize the app. The macOS CI leg runs it after producing
`target/cef-bundle/zz.app`. A local run proves the bundle currently in `dist/`; rebuild at the repo
root with `just build mac` after production changes before treating it as fresh evidence. Native GUI
rendering still needs visual smoke evidence; a clean headless summary must not be used as evidence
for that surface.

# Adding a scenario

Add a `.txt` file under `compat/scenarios/`. Keep each scenario focused on one behavior or one
stateful command family, and split it when independent setup or assertions could fail for unrelated
reasons. Long scenarios are appropriate when later assertions genuinely depend on the earlier
state. Put one tmux command on each line; the runner skips blank lines and lines beginning with `#`.
Use commands and flags that both command catalogs support, and target panes by index rather than by
raw `%N` IDs.

The runner handles shell quoting for command lines and rejects `$`, backtick, `;`, `&`, `|`, `<`,
and `>` before parsing them. Prefix a command with `zz-only:` or `tmux-only:` when a scenario needs
side-specific setup. A side-prefixed line skips the exit-class comparison for that step, but the
query trio still runs afterward.

Use `fmt: <format>` for a shared format assertion. The runner passes the payload as one argv value
to `display-message -p` on each side, without `eval`. This path accepts `#{}`, `?`, commas, colons,
semicolons, comparison and logic operators, and `/` delimiters. It rejects an empty payload, `$`,
backticks, either quote character, and `#(`. The `#(` guard prevents a tmux format from starting a
shell command during the differential run.

Use `out: <command...>` for a shared query whose own stdout is the assertion, such as
`out: show-options -gv @plugin`. It uses the same no-eval guards as `fmt:` and splits the payload
into one argv entry per whitespace-delimited token, so quotes, `$`, backticks, and `#(` are rejected.
Put values requiring spaces into an earlier ordinary setup command, then query them by name.

After each line, the harness runs the query trio. Scenario files should contain state changes plus
explicit `fmt:` or `out:` assertions, not ordinary `list-*` assertions whose stdout is ignored.

## Registering a discovered gap

Register a gap before implementing it:

1. Reproduce the behavior against the fetched pinned binary and identify the upstream command,
   option, format, hook, key, presentation rule, or model that owns it.
2. Add one stable ID to `compat/tmux-gaps.json`. Follow the existing entry shape and record the
   decision, status, priority and ease, owning subsystem, affected workflow, `depends_on` ordering,
   source evidence, and acceptance evidence. Keep the ID when status changes and update
   `updated_on` with the manifest.
3. Add the smallest failing test or differential scenario that proves the observation. Use a
   `known/` scenario only for an accepted exact mismatch. Its first metadata comment must be
   `# gap: <stable-gap-id>`, and the registry entry must declare the expected
   `TOPO GEO FMT OUT WARN` tuple.
4. Run `just compat-check`. Fix unclassified structural gaps, stale manifest entries, broken
   evidence, and tuple mismatches before changing behavior.
5. Implement the slice and run its focused evidence. Run the full strict corpus when the change can
   affect shared command, topology, geometry, format, output, config, or attached-client behavior.
6. If the implementation closes an adopt gap, pass its acceptance checks, then move the ID from
   `gaps` to `closed`. Record its title, `closed_on`, evidence, and resolution. If work remains,
   update the same active ID and its evidence. Regenerate `knowledge/tmux/gaps.md`, then run
   `just compat-check` again.

Use the generated report to choose the next slice. The roadmap supplies dependency order, and the
divergence matrix supplies detailed rationale; neither owns live status.

## Adding a smoke scenario

Add smoke configs and fixtures under `compat/scenarios/smoke/`. The smoke class boots both daemons
with a scratch HOME and prepends a generated executable `tmux` wrapper to PATH. The pin wrapper
executes the reference binary with `-L <label>`; the zz wrapper executes `zz --socket <path>`.
This makes literal `tmux` calls inside plugins hit the intended server on both sides.

The smoke directives are:

- `corpus: none` marks a self-contained smoke; `corpus: required` permits fixtures to use the eight
  pinned plugin checkouts. Every smoke scenario declares exactly one of these values.
- `conf: <path>` stages and sources a config after linking cached plugins into
  `~/.tmux/plugins/<name>`. A `~/`-prefixed path resolves against the scratch HOME, so a
  corpus file can be staged verbatim (`conf: ~/.tmux/plugins/oh-my-tmux/.tmux.conf`) —
  needed when a config locates itself as `~/.tmux.conf`, as Oh My Tmux does.
- `stage: <source> <destination>` copies one file into the scratch HOME before sourcing
  (both paths accept the same `~/` resolution; the destination must be under `~/`). Oh My
  Tmux uses it for its stock `.tmux.conf.local`.
- `expect-warn: zz <text>` and `expect-warn: tmux <text>` pin each side's
  `%config-error` set. Do not cross-diff skip summaries: they intentionally have no pin analogue.
  The harness separately requires the current tier-1 config loads to finish with `%end` on both
  sides and fails if either source-file block ends with `%error`.
- `keys: <table> <key>` compares only that binding through
  `list-keys -F '#{key_table}|#{key_string}|#{key_repeat}|#{key_command}'`. Stock tables differ,
  so whole-table comparison is invalid.
- Existing `out:` and `fmt:` directives remain available for option, environment, and format
  readback.

Capture stdout and stderr separately for every smoke command. Merging them with `2>&1` introduces
pipe-buffering order artifacts. The harness exports `ZZ_SMOKE_CANARY` into both daemon environments;
scenarios must never read it, which keeps the known clean-environment divergence from becoming an
implicit dependency.

Traps that produce false divergences:

- Every `new-window` needs `-n <name>`. Default window names are process-derived in tmux —
  and refreshed by the `automatic-rename` timer roughly 500ms later — but index-derived in zz.
  The runner's prologue renames window 0 to `main` on both sides for the same reason.
- Never kill scenario session `w`. The post-step TOPO, GEO, FMT, and OUT probes target `w`, so
  removing it turns every later probe into a fixture failure. Both sides create `w` explicitly;
  there is no auto-created session to remove.
- Never put `#{buffer_full}` in a differential scenario. `display-message -p
  '#{buffer_full}'` crashes the pinned tmux server; this is a verified pin trap, not a zz failure.
- `display-message` gets only tmux's newest automatic paste buffer. A named-only `set-buffer -b`
  setup therefore makes every `buffer_*` value empty on the pin. Add an automatic buffer for a
  `fmt:` probe; use `list-buffers -F` when the named row itself is what needs testing.

## Known divergences

Put a scenario with an accepted strict mismatch under `compat/scenarios/known/`. The runner
still executes every step and writes its diffs. It accepts the result only when the scenario's gap
ID resolves to the exact registered `TOPO GEO FMT OUT WARN` tuple. An unregistered known scenario,
a missing tuple, or any tuple drift fails the run.
The two current entries pin the deliberate refusals of upstream layout bugs:
`known-main-preset-two-panes.txt` (the pin never sizes the lone "other" pane) and
`known-spread-mixed.txt` (the pin's `-E` corrupts a parent mixing leaf and node children).
They use `layout.main-horizontal-upstream-bug` and `layout.spread-mixed-upstream-bug`.

Inspect a registered tuple directly with:

```sh
python3 compat/tmux-tracker.py known-tuple known/known-main-preset-two-panes.txt
```

Keep the `known/` set narrow. Move a scenario into the normal corpus when zz closes the gap. The
tracker rejects known-scenario evidence that does not match its registry entry.

`aggressive-resize.txt` covers stored option readback only. The harness has one short-lived CLI
client per side, so multi-client viewer selection belongs to daemon and convergence tests rather
than this corpus.

# Key files

| File | Role |
| --- | --- |
| `compat/check.sh` | Runs the oracle, registry, and full `zz-mux` library gate |
| `compat/tmux-gaps.json` | Owns active gaps, product status, ordering, evidence, and closed history |
| `compat/tmux-oracle.json` | Records schema 4 source and runtime inventories from the pin |
| `compat/tmux-oracle.py` | Captures and verifies the oracle from a clean pinned source checkout |
| `compat/tmux-tracker.py` | Validates the registry and generates the readable gap report |
| `compat/run.sh` | Builds both binaries and selects scenarios; a full run with `--attached-client` writes the canonical combined summary |
| `compat/startup-diagnostics.sh` | Runs the checksum-attested seven-case startup-cause differential without updating the canonical summary |
| `compat/fetch-tmux.sh` | Acquires tmux and validates its source-aware build stamp |
| `compat/fetch-corpus.sh` | Acquires and verifies the pinned plugin corpus |
| `compat/diff-scenario.sh` | Runs one scenario and emits per-step TOPO, GEO, FMT, OUT, and WARN diffs |
| `compat/scenarios/` | Holds the shared, smoke, and known-divergence corpora |

# Related

- [live tmux compatibility gaps](/tmux/gaps.md) . generated from the canonical registry
- [tmux drop-in plan](/designs/tmux-drop-in.md) . phase ordering and compatibility target
- [tmux divergence matrix](/tmux/divergences.md) . gaps the harness can turn into fixtures
- [updating the tmux reference](/playbooks/updating-tmux-reference.md) . how to move the pin
