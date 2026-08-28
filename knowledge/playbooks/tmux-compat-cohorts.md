---
type: Playbook
title: Running tmux compatibility cohorts
description: A bounded, parallel workflow for closing the practical alias tmux=zz gap without letting new oracle findings extend one campaign forever.
tags: [tmux, compatibility, campaign, workflow, agents]
timestamp: 2026-08-27T00:00:00-03:00
last_updated: 2026-08-28
last_updated_by: Codex
---

# Outcome

Close the practical `alias tmux=zz` gate through bounded slices. Each slice starts with a fixed
acceptance contract, ends with one reviewed commit, and leaves later discoveries in the
[live tracker](/tmux/gaps.md).

The Alert cohort completed in commit `2e4ccf3b9b6706e44215d74ca147643e6baa3d2e`. The dedicated
campaign branch then closed session cwd in
`2468bfd8f1a11430a73b7066b022101b4048d981`, requested client flags in the next milestone, and
retained-client sizing in the third milestone. The fourth milestone closed client-environment
seeding and refresh with protocol v82. The fifth closed retained client formats with protocol v83,
the sixth closed the six client lifecycle hook producers, and the seventh proved silent Control
delivery for asynchronous copy-pipe failures. The eighth registered and executed the 24-command
daemon invalid-flag runtime roster. The ninth closed all eight pinned positional-maximum mismatches
with catalog-owned metadata and the pin's first-positional flag boundary. The tenth closed all
fourteen required positional minima through a separate catalog sidecar. The eleventh removed the
partial maximum roster and applied the catalog contract to all 72 implemented finite upstream
commands, including stored binding and hook children. The complete CLI and app-library gates then
closed native `attach-session -E` routing, published `client-PID` targeting, and stale client-format
and command-palette assertions. The twelfth error-contract milestone replaced the partial daemon
flag roster with one catalog parser across all 83 implemented upstream commands and 74 aliases.
The thirteenth closed the final `mux.error-shapes` item by matching nested `new-session`
validation precedence without implementing session groups. The fourteenth closes the first custom
argument rule with protocol v84: `CommandInvocation` retains lexical command-block positions and
`if-shell` applies them across source-file, Control, aliases, bindings, hooks, and background work.
The fifteenth reuses that v84 metadata without another wire change and applies `run-shell`'s
leading `-C` rule across every positional, option boundary, stored command, Control route, and
background callback.
The sixteenth reuses protocol v84 again and applies the shared `SetOptionValue` rule to
`set-option` and `set-window-option`, including recursive group printing, format order, aliases,
stored commands, source-file, and direct Control behavior.
The seventeenth reuses protocol v84 for `bind-key`, applying the shared commands-or-string rule to
every positional while preserving the pin's distinct typed-tail, string-tail, key-printing, and
physical-group execution contracts through a real attached client.
The eighteenth reuses protocol v84 for `confirm-before`. Its one command positional now follows
the typed-block-or-string rule, option values remain strings, typed construction precedes target
lookup, and string construction follows parent-format expansion. Every lexical typed block
constructs recursively before parent name, callback type, or arity validation. Recursive paths
carry independent one-layer user-alias budgets, alias-produced subtrees disable further user
aliases, and self-recursion fails unknown without killing the daemon. Nested callbacks print
canonically, including `{  }` for an empty block and ` ;; ` for physical internal group newlines.
Stored `bind-key` and `set-hook` lists and typed `if-shell`, `run-shell`, and `confirm-before`
callbacks execute their constructed commands without another user-alias lookup. Typed `if-shell`
and `run-shell` callbacks preserve physical groups: a failed group stops its remaining commands
while later physical lines continue; string callbacks remain one group. `set-hook` and
command-valued native set-option deliberately construct again. Built-in hook values flatten
physical groups during their second pass, while custom `@` typed values retain textual ` ;; `
groups. A typed ignored `set-hook -R` value still constructs. A typed `display-menu` action drops
its structural wrapper before the fresh selection parse, while a quoted brace string remains
literal. Typed `command-prompt` templates retain their structured prepared command list through
submission without re-expanding aliases. Structured substitution preserves leaf-argument
boundaries against quote or semicolon injection. String templates substitute raw source before a
fresh parse and complete construction pass against the current alias table. Both paths replace the
first `%%` and every `%1`, with trailing-percent quoting. Typed callbacks retain physical groups,
while string templates and free input form one group. Prompt chaining and multi-answer `%2` retain
their existing prompt owner. The strict fixture proves exact construction,
parser, readback, and source-file plus Control channels; reply and `-y` Enter-default behavior
remain covered by daemon and GPUI tests.

The nineteenth milestone reuses protocol v84 for `command-prompt`. The command accepts zero or one
template positional as a typed block or string while `-I`, `-p`, `-t`, and `-T` values remain
strings. Typed templates preserve their recursively constructed commands through submission.
String templates preserve raw source, substitute the answer, then parse and construct every
resulting command before effects. Frozen typed aliases and fresh string aliases match the pin.
Source paths and lines survive the string path. The strict 43-check fixture drives a real attached
client through template typing, construction precedence, aliases, placeholders, injection,
physical groups, source-file diagnostics, and exact Control frames.

The twentieth milestone reuses protocol v84 for `set-hook`. Without `-B`, only value position 1
accepts a typed block or string. With `-B`, every positional lexically accepts either type, while
option values remain strings. Without `-B`, hook names and extra positionals also remain strings.
zz still rejects `-B` because format-monitor runtime behavior remains unsupported. Every typed child constructs
before parent type, arity, or effects. Typed values normalize through built-in hook, custom `@`, and
named-option forwarding paths. Built-in hooks flatten physical groups during a second construction
pass; custom `@` typed values retain textual ` ;; ` groups. An empty or failing local append creates
an empty local array that shadows the inherited global hook. The strict 24-check fixture covers
replacement, empty-value, and local-inheritance ordering, quoted braces, `-R`, aliases, stored bindings,
`default-client-command`, source-file diagnostics, and exact Control framing.

The twenty-first milestone reuses protocol v84 for `display-menu`. Its parser walks repeated NAME,
KEY, and ACTION fields. A nonempty NAME consumes a string KEY and a string-or-typed ACTION before
resetting to NAME; an empty NAME is a separator that consumes no KEY or ACTION. All ten valued
flags remain strings. Typed children construct before parent type, arity, or effects. Stored
bindings print canonical child commands for typed actions and preserve quoted action strings.
Incomplete NAME and NAME-plus-KEY tails construct and reach daemon runtime. Runtime resolves the
current or `-c` target client before completeness, so an unattached command or initial Control
reports `no current client`; initial Control uses a flag-0 `%error` and exits 1. Once attached,
Control validates an incomplete group as `not enough arguments` before its overlay no-op and emits
an exact flag-1 `%error`; EOF after that frame exits 1. Interactive menu ordering is unchanged. The
strict three-step, 34-check fixture covers state transitions, type boundaries, all valued flags,
aliases, stored readback and preservation, client-before-completeness precedence, incomplete
runtime groups, source-file diagnostics, and PID-unique FIFO Control framing.

The twenty-second milestone reuses protocol v84 for `display-panes`. Its optional template accepts
a string or typed block while `-d` and `-t` values remain strings. Targetless routing resolves an
attached client before duration validation. The strict three-step fixture runs 22 internal checks
with zero differential channels. Custom selection-template execution remains parked under
`display-panes.command-template`.

The twenty-third milestone closes `choose-buffer` and `choose-tree` together. This is the deliberate
exception to the planned one-command 10j and 10k split: both commands use the same
commands-or-string callback, the same chooser-template preparation and selection executor, and one
26-check attached-client fixture. Splitting them would duplicate the production change and the
proof without producing an independent closure. Both commands accept zero or one string-or-typed
template. Typed templates freeze constructed aliases before opening; string templates parse
against the current alias table after selection. The daemon closes the chooser first, substitutes
the exact buffer name or tree target, and executes against the invoking client's live context.
Direct and stored commands now validate positional bounds before rejecting a recognized parked
capability. The tracker has no remaining command-specific `args-parse:` item.

The twenty-fourth milestone closes slice 10l as a source registration. A daemon-owned invariant
partitions all 68 pinned hook names into 27 explicit event producers, 37 generic
`after-<command>` producers derived from implemented canonical commands, and four active gaps:
`after-queue`, `pane-focus-in`, `pane-focus-out`, and `pane-set-clipboard`. The test rejects
duplicate explicit names and overlap between produced and tracked names. `just compat-check`
requires the named daemon test and runs it through `--exact`. The slice changes no runtime
behavior, protocol, differential scenario, or step.

`clients.attach-context` closed as three bounded contracts. Sessions keep one internal cwd, and
attached source loading prefers it. Clients keep requested flags through attach, switch, detach,
and TUI reconnect. `resize-window -A` and `-a` now aggregate retained client geometry once and
freeze the result as manual sizing. None of the three slices changed the wire or snapshot schema.
Clients now add one bounded environment snapshot to the handshake. Fresh sessions, existing
attach, native attach, Control attach, and targeted switch apply the pinned `update-environment`,
wildcard, missing, empty, hidden, `-A`, `-e`, `-E`, and `-T` rules. Session values survive client
disconnect, future panes read updates, and existing processes keep their startup environment.
One shared retained client-fact record now serves list rows, ordinary and inserted commands, status
recipients, and `display-message`; the attached fixture covers Interactive and Control empty
behavior against the pin.
Per-window latest-client retention now drives `client-active` only on ownership changes, while
focus, theme, and positive Interactive size reports retain the pin's duplicate behavior. Hook
replay stays clientless, `hook_client` names the reporter, and the reporter's copied session,
window, and shared active pane supply ordinary target formats. Control clients originate none of
those report hooks but remain eligible for promotion after the latest client leaves.
Changed-resize post-geometry format timing remains a separate protocol-owned slice so the producer
milestone does not grow across the TUI message boundary.
`active-pane` and `no-detach-on-destroy` are retained and reported, but their consumers remain
explicit later gaps.

The combined 10j/10k canonical checkpoint covers 98 scenarios and 1,517 steps. Every ordinary row
is clean.
`known/known-main-preset-two-panes` and `known/known-spread-mixed` each retain exactly one documented
GEO divergence with every other channel clean. The sizing milestone's expanded multi-client
attached fixture passes, and `compat/run.sh --check-summary` confirms the canonical summary SHA-256
is
`9c147eb5caa78ca51e068275b28836ab2647d3d959d047c5fafbcb5c0bf86832`.
The 10l source-registration milestone leaves that artifact and digest unchanged.
The attached-client result is `PASS`, every ordinary row is clean, and only the two registered GEO
rows remain. The historical 10i checkpoint remains 97 scenarios and 1,514 steps at SHA-256
`3b728eb8f0d30cae1bf1fe9c09100188279aaf8c80c0b33b30cd15b617f75d70`.
The historical 10h checkpoint remains 96 scenarios and 1,511 steps at SHA-256
`75aee7176d3ed3cf1886d4f4c697062089b87644036e85f0230f355fac7d4217`.
The historical 10g checkpoint remains 95 scenarios and 1,508 steps at SHA-256
`15385526cd2098f35276c27cd8edfef338569cd6a6c87ffe80d8f919701f042a`.
The historical 10f checkpoint remains 94 scenarios and 1,505 steps at SHA-256
`31b03805b5701aff0555ebe4d4b40a0116b8525130d4d3406963e9a1c8f1919c`.
The historical 10e checkpoint remains 93 scenarios and 1,502 steps at SHA-256
`e0783568fc5845eaaa9ff4b84256d43a046ced996fbf8b664bc65d9bf0d9578a`.
The historical 10d checkpoint remains 92 scenarios and 1,499 steps at SHA-256
`afea2249cd62402fe00dc8c54ea60662eb616ef584806f4774cd77723746144e`.
The combined 10j/10k artifact comes from a fresh full strict suite run. Its chooser row contributes
three harness steps and 26 internal checks with zero TOPO, GEO, FMT, OUT, or WARN differences. The
historical 10i display-panes row contributed three harness steps and 22 internal checks with the
same clean channels.
The two positional-bound scenarios prove canonical and alias diagnostics, the
first-positional flag boundary, target-error precedence, and effect suppression. The expanded
maximum fixture covers 71 generic-CLI-routed canonical names and 62 aliases; Rust coverage
also includes the exact attach engine path and stored commands. The focused three-step
`command-flag-errors` run is clean on zz and the pin with 516 byte-compared probes inside the
canonical suite. The focused three-step `args-parse-if-shell` row runs 12 internal source-file,
Control, alias, stored-command, and branch-selection checks.
The focused three-step `args-parse-run-shell` row runs 21 internal checks for lexical type,
combined and boundary flags, aliases, foreground and background execution, Control, and stored
command preservation.
The focused three-step `args-parse-set-option` row runs 21 internal checks across both set-option
commands for lexical type, recursive group printing, format order, aliases, source-file, Control,
and rejected-state preservation.
The focused three-step `args-parse-bind-key` row runs 16 internal checks for typed option and key
positions, exact typed and string tails, aliases, boundary flags, stored-child preservation, and
physical-group execution through a real attached client.
The focused three-step `args-parse-confirm-before` row runs 19 internal checks for recursive typed
and string construction, string-only option values, canonical nested readback, per-path alias
budgets, self-recursion safety, physical groups, target and child diagnostics, exact source-file
and Control channels, and rejected-binding preservation. Nested bind and confirm failures are
preflight parse errors. The constructed confirm callback remains frozen through execution; stored
bindings and hooks likewise perform no execution-time user-alias lookup. It does not claim replies
through raw zz-tui, which does not yet consume confirm, menu, or popup state, and it leaves eager
whole-file source construction plus the broader replay-channel placement difference open.
Late focused regressions also prove that typed `if-shell`, `run-shell`, and structured
`command-prompt` callbacks stop the failed physical group and continue later physical lines, while
string callbacks remain one group. Structured prompt substitution preserves leaf-argument
boundaries against quote or semicolon injection. Raw string templates substitute before parsing
and complete construction. Both paths replace the first `%%` and every `%1`, with trailing-percent
quoting.
Typed `display-menu` actions lose their structural wrapper before the fresh selection parse, while
quoted brace strings remain literal. The later item-rule closure keeps incomplete NAME and
NAME-plus-KEY groups at daemon runtime. Runtime resolves the current or `-c` target client before
completeness. Initial Control returns a flag-0 `no current client` error. Attached Control validates
an incomplete group as `not enough arguments` before its overlay no-op, returns a flag-1 `%error`,
and exits 1 after EOF. Prompt chaining and multi-answer `%2` retain their existing prompt owner.
The focused three-step `args-parse-command-prompt` row drives a real attached client and runs 43
internal checks across template types, recursive construction precedence, alias timing,
substitution, injection resistance, physical groups, source-file diagnostics, and exact Control
frames. Both sides finish with `ARGS_PARSE_COMMAND_PROMPT=clean:43`.
The focused three-step `args-parse-set-hook` row runs 24 internal checks across lexical types,
child-before-parent precedence, canonical readback, preexisting aliases, physical groups,
built-in versus custom storage, replacement, empty-value, and local-inheritance ordering, `-R`, named-option
forwarding, stored bindings, and exact Control framing. Both sides finish with
`ARGS_PARSE_SET_HOOK=clean:24`. The fixture leaves eager whole-file construction, same-source alias
mutation, multiline inner-source placement, `-B` monitor semantics, and broader replay placement
with their existing owners.
The focused three-step `args-parse-display-menu` row runs 34 internal checks across the repeated
NAME, KEY, and ACTION state, empty-name separators, typed and quoted actions, all ten string-only
valued flags, child construction precedence, canonical, built-in alias, prefix, and preexisting
user-alias paths, stored binding readback and preservation, incomplete runtime groups, source-file
diagnostics, and exact initial flag-0 plus attached flag-1 Control frames through a PID-unique FIFO.
Both sides finish with `ARGS_PARSE_DISPLAY_MENU=clean:34` and zero differences. Attached rendering
and input, geometry, styles, targets,
formats, selected-action runtime errors, same-source alias mutation, eager whole-source
construction, generic alias recursion, and raw-TUI overlay parity retain their owners.
The focused three-step `args-parse-display-panes` row runs 22 internal checks across the optional
string-or-typed template, string-only `-d` and `-t` values, child-before-option-type and arity validation,
canonical, built-in alias, prefix, and preexisting user-alias readback, targetless client routing,
duration precedence, source-file, and direct Command-client runtime paths. Both sides finish with
`ARGS_PARSE_DISPLAY_PANES=clean:22` and zero TOPO, GEO, FMT, OUT, or WARN differences. Custom
template execution remains parked because mux runtime rejects a positional value instead of
substituting the selected `%pane` for `%%%` and executing with the original queue state. Tmux uses
`select-pane -t "%%%"` when the template is omitted; queue blocking and presentation keep their
existing owners.
The focused three-step `args-parse-choosers` row runs 26 internal checks across `choose-buffer` and
`choose-tree`. It covers string-only option values, typed and string alias timing, exact `%1` and
`%%` substitution with trailing-percent quoting, live invoking-client context, tree target
spellings, stale and empty buffers, chooser-close ordering, uppercase attached errors, and direct
plus stored arity precedence over recognized parked flags. Both sides finish with
`ARGS_PARSE_CHOOSERS=clean:26` and zero TOPO, GEO, FMT, OUT, or WARN differences. Broader chooser
flags, presentation, eager whole-source construction, same-source alias mutation, generic alias
recursion, and raw-TUI overlay parity retain their owners.

# Cohorts

| Phase | Tracker scope | Dependency | Exit proof |
|---|---|---|---|
| Alert | Closed alert groups | Complete | Focused daemon and terminal tests, pinned alert probes, one full debug attached-client fixture, tracker and knowledge updates |
| Client foundation | Session cwd, requested flags, sizing, environment, formats, and `clients.event-hooks` closed | Complete | One written oracle contract per slice, focused differential coverage, and one full debug attached-client fixture per milestone |
| Error contracts | Async copy-pipe, shared arity, shared flag diagnostics, and nested `new-session` precedence closed; `tracker.semantic-coverage` remains | Independent of Client foundation except where a proof names client context | Every changed claim gets a pinned differential or a focused test with a named tracker item, followed by one full debug attached-client fixture |
| Copy behavior | `copy-mode.action-fidelity`, `copy-mode.command-fidelity`, `keys.copy-mode-binding-fidelity`, `keys.copy-mode-unsupported-default-actions`, `keys.copy-mode-prompt-defaults` | Command fidelity requires `clients.interactive-refresh`; prompt-backed defaults also require `prompt.command-fidelity` | Source-owned action and binding inventories, attached key-path probes, and one full debug attached-client fixture |

These phases are navigation, not commit boundaries. One persistent goal and one milestone commit own
one bounded slice. Split a tracker group before implementation when its acceptance contract crosses
unrelated production paths. Do not merge slices to save a commit.

# Selected dependency-ordered tranche

The queue separates execution order from apparent ease. A blocked medium item does not jump ahead of
the hard state contract that makes its proof meaningful. A range such as `10a-10f` means one
milestone per letter unless a row records a shared-rule exception. The combined 10j/10k chooser
closure is that exception.

| Order | Slice | Current tracker ownership | Relative effort | Why it is bounded |
|---|---|---|---|---|
| 1 | Session cwd and attached `source-file` cwd | Closed under `clients.attach-session-cwd` on 2026-08-26 | Complete | One internal session-state path; no client-environment or format vocabulary |
| 2 | Requested client flags | Closed under `clients.attach-flags` on 2026-08-27 | Complete | One attach-state contract; establishes `ignore-size` |
| 3 | Largest and smallest client sizing | Closed under `clients.attach-sizing` on 2026-08-27 | Complete | One-shot component-wise aggregation, manual freeze, global `ignore-size` fallback, Control ceilings, and default fallback; no wire change |
| 4 | Client environment seeding and refresh | Closed under `clients.attach-environment` on 2026-08-27 | Complete | Protocol v82 plus one per-connection snapshot; exact and wildcard refresh semantics remain session scoped |
| 5 | Client format facts | Closed under `clients.context-formats` on 2026-08-27 | Complete | Protocol v83 plus one retained-fact contract across list, status, Control, inserted, and targeted contexts |
| 6 | Client lifecycle hook producers | Closed under `clients.event-hooks` on 2026-08-27 | Complete | Per-window latest ownership plus five report boundaries; no protocol change |
| 7 | Interactive refresh decision | `clients.interactive-refresh` | Hard decision gate | Either justify and adopt the cross-client mode contract or keep it parked and reclassify dependent copy claims |
| 8 | Async copy or pipe error delivery | Closed under `control-mode.async-copy-pipe-errors` on 2026-08-27 | Complete | Pinned source plus a delayed exit-7 Control probe prove silent delivery and copy-mode cancellation |
| 9a | Daemon invalid-flag runtime contract | Closed under `tracker.daemon-invalid-flag-runtime` on 2026-08-27 | Complete | Initial 24-command production-dispatch proof; the later shared parser superseded the partial roster |
| 9b | Initial positional maximums | Closed under `mux.positional-maximums` on 2026-08-27 | Complete | One catalog validator across the first eight commands, with mux and daemon production-boundary proof |
| 9c | Required positional minima | Closed under `mux.positional-minimums` on 2026-08-27 | Complete | One catalog sidecar and validator after flags but before targets and effects |
| 9d | Shared arity errors | Closed under `mux.command-arity-errors` on 2026-08-27 | Complete | All implemented finite upstream commands plus stored children, without absorbing inner callback grammar |
| 9e | Shared flag errors | Closed under `mux.command-flag-errors` on 2026-08-28 | Complete | One catalog parser across 83 canonical commands and 74 aliases; 516 differential probes cover exact diagnostics and value boundaries |
| 9f | Nested `new-session` error precedence | Closed under `mux.error-shapes` on 2026-08-28 | Complete | Separate client-lifecycle path with its own oracle proof |
| 10a | `if-shell` branch argument rule | Closed under `tracker.args-parse-if-shell` on 2026-08-28 | Complete | Protocol v84 preserves typed blocks; one command and one effective rule |
| 10b | `run-shell` command-mode argument rule | Closed under `tracker.args-parse-run-shell` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; one command and one effective rule |
| 10c | Shared set-option value argument rule | Closed under `tracker.args-parse-set-option` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; two commands and one effective rule |
| 10d | `bind-key` commands-or-string argument rule | Closed under `tracker.args-parse-bind-key` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; one command within a shared rule |
| 10e | `confirm-before` commands-or-string argument rule | Closed under `tracker.args-parse-confirm-before` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; recursive construction and exact channel proof stay separate from client reply rendering and whole-file source construction |
| 10f | `command-prompt` commands-or-string argument rule | Closed under `tracker.args-parse-command-prompt` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; one typed-or-string template plus deferred substitution, alias, source, and group boundaries |
| 10g | `set-hook` monitor-or-value argument rule | Closed under `tracker.args-parse-set-hook` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; lexical `-B` typing is closed while unsupported monitor runtime behavior retains its owner |
| 10h | `display-menu` repeating item argument rule | Closed under `tracker.args-parse-display-menu` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; data-dependent NAME, KEY, and ACTION typing closes without absorbing menu presentation or selected-action execution |
| 10i | `display-panes` commands-or-string argument rule | Closed under `tracker.args-parse-display-panes` on 2026-08-28 | Complete | Protocol v84 metadata reused without a wire change; parsing and client-routing precedence close while custom selection-template execution remains parked |
| 10j/10k | `choose-buffer` and `choose-tree` commands-or-string rule | Closed under `tracker.args-parse-choose-buffer` and `tracker.args-parse-choose-tree` on 2026-08-28 | Complete | One deliberate combined milestone for one callback rule, one chooser-template executor, and one attached 26-check proof |
| 10l | Hook-producer source registration | Closed under `tracker.hook-producer-partition` on 2026-08-28 | Complete | 27 explicit plus 37 derived producers reconcile with four tracked hook gaps; no runtime or differential change |
| 10m-10p | Remaining source-owned tracker registrations | Shared key behavior next, then nonconstant formats, open context formats, and option consumers, one semantic item per slice | Small to medium | Four unrelated owners remain four independent milestones |
| 10q-10s | Raw TUI daemon overlays | Three items in `clients.tui-overlay-consumption`, one confirm, menu, or popup surface per slice | Hard | ClientCore already retains state; each client renderer and input contract remains independently closable |
| 11 | Copy action vocabulary inventory | `semantic:copy-mode-action-vocabulary` in `copy-mode.action-fidelity` | Small research | Record and classify all 95 pinned actions before behavior changes |
| 12a-12f | Copy action behavior | The other six `copy-mode.action-fidelity` semantics, one category per slice | Hard | Cursor, logical-line, goto, selection, jump/prompt, and copy effects stay independently provable |
| 13 | Unsupported stock action bindings | `keys.copy-mode-unsupported-default-actions` | Medium after slice 12 | Seven keys become honest only after their five actions exist |
| 14 | Copy command fidelity | `copy-mode.command-fidelity` | Hard | Requires the interactive-refresh decision |
| 15 | Shared copy binding fidelity | `keys.copy-mode-binding-fidelity` | Hard | Follows command fidelity; owns exactly 15 divergent command shapes |
| 16 | Generic prompt command fidelity | `prompt.command-fidelity` | Hard | Requires the interactive-refresh decision and remains broader than copy mode |
| 17 | Prompt-backed copy defaults | `keys.copy-mode-prompt-defaults` | Medium after slice 16 | Ten defaults land only after their generic prompt contract |

Slices 9a through 9f and 10a through 10l are closed; shared key behavior in slice 10m is next. Before choosing each later milestone,
regenerate the report
and re-rank every active daily, script, remote, or silent-mismatch group. That audit must include
attach-dependent work such as `buffers.client-file-context`, the three open `source-file.*-client-cwd` groups,
`clients.detach-exec`, `clients.parent-hup-exit`, and `clients.tui-overlay-consumption`. Rows 4 and
later are a dependency forecast,
not permission to skip a newly unblocked practical gate. Keep formats, hooks,
`active-pane`, and `no-detach-on-destroy` as separate slices.

# Four-seat Codex pipeline

Use the four seats as one coordinator and three Codex subagents:

1. The coordinator fixes the slice boundary, assigns file ownership, integrates changes, and owns
   the commit.
2. The oracle agent probes the pinned tmux commit and writes the acceptance contract plus the
   smallest differential fixture that can disprove it.
3. The implementation agent changes one owned subsystem and runs focused tests. After review starts,
   this seat may scout the next slice without editing its files.
4. The review agent hunts context, performs an independent code and proof review, then checks
   tracker and knowledge claims against source.

Assign one owner to each path before agents edit. The coordinator resolves overlaps instead of
letting two agents rewrite the same file. Use Codex subagents for this campaign.

# Validation ladder

Run the cheapest proof that can fail the current edit:

1. During implementation, run focused Rust tests and the scenario or attached probe for the changed
   behavior.
2. At slice close, build the debug binary and run the full attached-client fixture against the
   pinned tmux oracle. Treat a skip or reduced scenario count as a failure.
3. At a campaign checkpoint, run `just compat --strict-geometry --attached-client`, regenerate the
   canonical summary, and run `compat/run.sh --check-summary` as a separate check.

Use campaign checkpoints after the Alert cohort, after two more completed slices, and at the
practical exit gate. Run one earlier if a change invalidates the stored checkpoint. Do not run
release builds for compatibility work.

# Discovery rule

The oracle agent records new gaps in `compat/tmux-gaps.json`. A discovery joins the active slice
only when it uses the same production path, needs no protocol or schema change, and fits the slice's
existing proof. A discovery that invalidates the slice's claimed behavior blocks closure. All other
findings wait for a later cohort.

Freeze the acceptance contract after the oracle and implementation agents agree on it. Review can
reject the implementation or proof, but it cannot expand the slice with unrelated cleanup.

# Goal boundary

By default, create one persistent Codex goal per slice and name its exact tracker items and exit
proof. When Fabrico explicitly asks for the whole campaign to continue unattended, one campaign
goal may span the practical exit gate. The slice boundary does not change: freeze, prove, review,
document, and commit one milestone before starting the next one.

# Milestone commits and worktree

Commit each slice after code review, proof, tracker generation, and OKF validation pass. Stage exact
paths or hunks because the shared checkout may contain unrelated work. Ask before the first commit
unless Fabrico has already authorized continuous campaign commits. Do not push unless he asks.

Resolve the `codex/tmux-compat` branch and `/Users/demfabris/dev/zz-tmux-compat` path with read-only
checks first. If both are absent, create the dedicated worktree from the current clean campaign base
after verifying that the Alert commit is its ancestor. If either exists, inspect and reuse it safely;
never overwrite it. Leave unrelated shared-checkout edits intact.

# Practical exit gate

The campaign reaches compatible-enough status when all of these hold:

- Daily session, window, pane, config, plugin, Control, and attached-TUI workflows have no known
  silent semantic mismatch.
- The config and plugin corpus runs with no unexpected skip.
- The current strict differential and attached-client fixture pass at their full scenario counts.
- The canonical summary describes the current corpus rather than an older scenario set.
- Every remaining gap has an explicit `native`, `park`, or `never` decision, or produces a loud error
  that cannot corrupt state.

The tracker can retain long-tail work after this gate. It cannot retain an unclassified daily-use
surprise.

# Bootstrap prompt for the next session

Paste this prompt into the next session:

```text
Continue the tmux compatibility campaign in /Users/demfabris/dev/zz-tmux-compat on
codex/tmux-compat. Preserve unrelated work and do not push.

Verify that the session-cwd, requested-client-flags, retained-client-sizing, client-environment,
client-formats, client-hooks, asynchronous copy-pipe, daemon-invalid-flag, positional-maximum, and
positional-minimum milestones are committed and their tracker entries are closed. Also verify the
shared arity and shared flag closures plus the complete CLI and app-library gate repair. Confirm the
focused command-flag scenario reports 516 matching probes, then use the current canonical summary
for the full scenario count, attached-client result, and the two documented GEO rows.

Regenerate and re-rank the entire active tracker before selecting the next bounded slice. Include
daily, script, remote, and silent mismatches plus newly unblocked attach-dependent work. Freeze one
acceptance contract after that audit. Do not combine context formats, event hooks, exit actions,
`active-pane`, or `no-detach-on-destroy` behavior merely because they share
client state.

Read AGENTS.md, this playbook, the live tracker, the roadmap, the relevant OKF pages, and cited
source before editing. Use one coordinator and three Codex subagents to probe the selected
pinned-tmux behavior, trace its current owners, and design the minimum differential proof. Freeze
the contract before implementation and assign disjoint file ownership.

Run focused tests, build a fresh debug binary, and run the full attached-client fixture when the
slice touches attached clients. Rerun the canonical strict differential at the next campaign
checkpoint, when a change invalidates the artifact, or when a new canonical scenario joins the
corpus. Update the tracker
and OKF documents, validate them, get an independent review, and commit one milestone. Continue the
campaign goal into the next slice without pushing.
```
