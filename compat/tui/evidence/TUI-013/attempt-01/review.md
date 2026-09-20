# TUI-013 attempt-01 review

Independent adversarial review, run on the macbook on 2026-09-20 from the shared
checkout at `04a39258`, against the same `target/debug/zz_cli` the attempt names
and the same pin. Every number below is one I measured here.

## Verdict

approve: both acceptance clauses are met by evidence I reproduced on this box,
the attestation chain (revision, clean tree, binary hash and build time) holds,
and the record names no defect for the vanished binary.

## Confirmed defects

- nit, `notes.md` "How this ran" and the README close-out: the parenthetical
  explains the `run-2.js` lint failure with three rules. `lint-runner.py` exits 1
  with 13 of 16 rules failing for that file, among them zones-around-obligations,
  judged-on-flips, cheap-iteration, corpus-grep, held-proof, gate-order,
  last-gate-shared-corpus, self-check, compile-parallelism and every-fixture.
  Fix: write "13 of 16 rules fail, including the wire rule (99 against the tree's
  105), agent concurrency and the cargo caps" in both places.
- nit, `evidence_note` and `notes.md`: "a `target/debug/zz` that was unattested,
  never rebuilt" reads against TUI-001's own record that the shared checkout's
  `target/debug/zz` was rebuilt on this box at 18:25 on 2026-09-09. The two
  statements are about different moments and both are true, but a reader hits the
  contradiction before the resolution. Fix: "not rebuilt before that run and
  overwritten the same evening, so nothing about it can be measured now".

No blocker and no must-fix. In particular I looked for what clause 2 forbids and
did not find it: neither `evidence_note` nor `notes.md` names or implies a defect
in the vanished binary, and the stale-pre-fix-binary story appears only as the
one proposed mechanism, carried with its refutation from the registry.

## Checks run

- `git status --short`: exit 0. At the time of my reproduction the only paths were
  `compat/tui/evidence/TUI-013/`, `compat/tui/campaign.json` and
  `knowledge/tmux/tui-parity.md`. The worker later added `compat/tui/HANDOFF.md`,
  `compat/tui/README.md`, `knowledge/log.md` and a one-line FIXTURES mapping in
  `compat/tui/verify-claims.py`. No crate source is modified, so attestation is
  intact.
- `git rev-parse HEAD` and `git rev-parse origin/main`: both
  `04a39258974fed54a067df5ed27425b9faf128b4`, equal to `proof.revision` and to the
  revision in `environment.txt`.
- `git reflog`: HEAD reached `04a39258` at 14:09:39. `stat` puts
  `target/debug/zz_cli` at 14:16:20 and the first evidence file at 14:16:55, so the
  binary was built from this tree and before every run.
- `shasum -a 256 target/debug/zz_cli`: exit 0,
  `e4e85474cef55166008164fb3cfdbed9d76f8a1a8b3b96a4283b3222188214a4`, the hash in
  `environment.txt` and in the proof block. `./target/debug/zz_cli --version`:
  `zz 0.11.1`. No `target/debug/zz` exists here, as the notes say.
- `git -C compat/.cache/tmux-src rev-parse HEAD`: `d77c9dc6aa021e4bc61f0da128c591af695e6466`,
  equal to `proof.tmux_commit`. `compat/fetch-tmux.sh`: exit 0, cache verified.
  `shasum -a 256 compat/.cache/tmux-src/tmux` matches the recorded `f9f20dd7`.
- `compat/tui-pane-geometry.sh $PWD/target/debug/zz_cli <pin>`, three consecutive
  runs with the committed binary: exit 0, exit 0, exit 0 in 5.48 s, 5.48 s and
  6.70 s against the fixture's 10 s bound. All three stdouts are byte identical to
  `geometry-run-1.stdout.txt` (`diff -q`, no output), all three stderrs are 0 bytes,
  and no `/tmp/zzgeo-diag.*` directory is left.
- `compat/status-row.sh $PWD/target/debug/zz_cli <pin>`: exit 0 in 15.22 s, tally
  `all 14 comparisons identical, none recorded`, byte identical to
  `status-row.stdout.txt`. My shell carried `LANG=LC_ALL=en_US.UTF-8` with `LC_TIME`
  unset, the locale `environment.txt` records, so this run does not test the `%b`
  path TUI-001 hit and the notes say as much.
- Failure path, measured here rather than inherited from TUI-001: a copy of the
  fixture in my scratchpad with `file_has_two_fields` demanding three fields exits
  2 in 18.6 s with `wait that ran out: zz geometry report` and a 13 file dump
  (both outer screens, both sides' client and pane lists, daemon stdout and stderr,
  both clients' stderr, the ring log). A missing report fails this fixture on
  macOS with this binary, which is what clause 1 requires.
- `python3 compat/tui/verify-claims.py --run TUI-013 --zz target/debug/zz_cli`:
  exit 0, `all 6 asserted measurements identical`, `every verified obligation holds
  up`, byte identical to the committed `verify-claims.txt`.
- `python3 compat/tui/tracker.py check`: exit 0, before and after the worker's later
  edits. `python3 compat/tmux-tracker.py check`: exit 0.
- Artifacts: all eight paths in `proof.artifacts` plus `proof.review` exist. Nothing
  in the evidence directory is named `*.log`. A scan for `gho_`, `ghp_`, `AKIA`,
  `BEGIN ... PRIVATE KEY`, token, secret, password and api key across the directory
  returns nothing; `environment.txt` prints named variables only, not a dump.
- Registry: `tui.client-input-backpressure`'s resolution carries, verbatim, "It also
  exited 0 at origin/main 012b4dcc before the fix" and the outer-tmux drain
  mechanism, so clause 2's two supports are registry text and not this attempt's
  invention. `compat/tui/evidence/TUI-001/attempt-01/timeout-diagnostics/` holds the
  14 file sabotage dump the notes cite.
- Fixture binary name: `compat/run.sh` line 407 builds `-p zz-cli` and line 409 sets
  `ZZ_BIN="$REPO_DIR/target/debug/zz_cli"`, and TUI-014's proof block records its
  runs against `target/debug/zz_cli`. The attempt's choice of binary is the tree's.
- `crates/zz-mux/src/formats.rs:4976` `format_datetime` still parses through
  `chrono::format::StrftimeItems`, so the notes' caveat that the green status row
  measures the locale and not a fix is accurate.

## Notes

- The one timing divergence worth carrying: my third geometry run took 6.70 s
  against the 10 s bound while the other two took 5.48 s, under my own concurrent
  load. The file mtimes in the evidence directory put the worker's three runs at
  about 5.4 s each, so the recorded 5.5, 5.5 and 5.4 are honest, but the margin is
  roughly 1.5x rather than 2x once the box is busy. If this fixture ever expires
  again on a loaded macOS box, the retained dump is what decides whether it is a
  runtime fault or the bound.
- I did not re-run `compat/check.sh`, so the proof block's `compat/check.sh -> exit 0`
  is the only command there I have not independently measured. It matters because the
  records commit adds the TUI-013 mapping to `verify-claims.py`, which is what makes
  check.sh able to re-measure this obligation; the integrator should confirm check.sh
  was run after that line was added.
- I started nothing that outlives this review: no fixture sockets under
  `/tmp/tmux-$(id -u)`, no `/tmp/zzgeo*` paths and no `zz_cli` processes remain. My
  own diagnostics dump lives in my scratchpad, not in `/tmp` and not in the evidence
  directory.
- The worker was editing `campaign.json`, `README.md`, `HANDOFF.md`, `knowledge/log.md`
  and `verify-claims.py` while this review ran. Everything I verified above was
  re-checked against the later state, including the added `verify-claims.txt` artifact.
