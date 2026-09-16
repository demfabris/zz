export const meta = {
  name: 'tui-parity-cycle-11',
  description: 'Close the TUI parity campaign: TUI-014, TUI-015, TUI-017, TUI-018, then TUI-011',
  phases: [{ title: 'Lanes' }, { title: 'Review' }, { title: 'Gate' }],
};

// Cycle 11, alienware box, 2026-09-16. Six Codex CLI lanes (gpt-6-astra, reasoning high) on
// fabrico's instruction, the orchestrator reviewing and gating. The cycle resumes the three fix
// passes the ubuntu box left in flight and adds the customize-mode lane fabrico ruled in.
//
// Lanes are launched as detached `codex exec` processes, not by this file. This file is the
// cycle's record and the contract every lane prompt is generated from, and `lint-runner.py`
// checks it against every lesson an earlier cycle paid for.

const SLOTS = 3;        // at most three agents; the two cargo slots still cap compiles at two
// exitcode gates FIRST: main is red without it (three asserted cases in
// compat/tui-client-commands.sh), so no other branch can prove itself against a green main,
// and it touches catalog.rs and message.rs which modes-12 and alias-2 also carry.
const ORDER = ['exitcode', 'alias-2', 'modes-12', 'capture-12', 'customize',
               'capture-residuals', 'zz-knobs'];

const COMMON = `
ZONES. A lane's zones are drawn around its OBLIGATION, across whatever crates it needs, not
around a crate. Three cycles running, an obligation stayed open only because its last fix sat in
a file the lane did not hold. Take the file the fix needs and DECLARE the complete three-dot
footprint, mechanical test-helper churn included; an undeclared footprint is a reject.

JUDGED ON FLIPS. A lane is judged on recorded cases flipped to asserted, never on divergences
measured and recorded. Cycle 4 merged three lanes that verified nothing that way. Every flipped
case needs a matching --self-check sabotage, because a case that cannot fail proves nothing. An
obligation whose own recorded count reaches zero is what verified means.

ITERATE BEHIND A FILTER. One edit-check loop that runs a whole package suite costs minutes behind
the memory cap and the two shared slots. Use cargo test -p <pkg> --lib <name> while fixing, and
the full package only before the commit that closes an item.

CARGO. Every cargo command goes through /tmp/zz-cargo.sh, which wraps systemd-run --user --scope
with MemoryMax plus a flock on one of two shared zz-cargo-slot- locks. Five uncapped lanes drove
this box out of memory on 2026-09-11. compat/run.sh builds zz outside those slots, so lanes set
ZZ_COMPAT_ZZ to a built binary and it invokes cargo zero times.

CORPUS. A lane that removes or changes a zz-only screen string must grep compat/scenarios for it;
twice a presentation change landed that a corpus row still contradicted. Every lane and every
reviewer runs the DELTA CORPUS for its touched commands, and reports unrun rows by name rather
than implying coverage.

WIRE. PROTOCOL_VERSION is 103 in the tree and v0.10.0 SHIPPED it on 2026-09-16 at 00:28, so 103
is frozen history. The first wire append of this cycle opens 104: bump message.rs, open a new
v104 entry in knowledge/protocol/wire-protocol.md, and move both pins in
crates/zz-protocol/tests/hunt_claims.rs (0x67 becomes 0x68). Appends stay tail appends carrying
serde(default) so an older client keeps decoding. compat/wire-version.py fails closed on this.

ATTRIBUTION. compat/tui-client-commands.sh is shared by six obligations. Its case_owner table
names every recorded case's owner, the summary line ends unattributed=0, and a reason opening
with DECIDED is a clause-mandated registration filed under decided:<owner>. An unowned case is
charged to every obligation on the fixture.

DELIVERY. NO attribution trailers and no added code comments; the repo's CLAUDE.md forbids both,
and a review counted 160 added comment lines as a must-fix. knowledge/tmux/gaps.md and
knowledge/tmux/tui-parity.md are generated and are only ever regenerated.
`;

// 1. The lanes, in the order their branches became available.
const LANES = [
  {
    key: 'alias-2', obligation: 'TUI-018', from: 'campaign/tui-stream-alias-gated',
    push: 'campaign/tui-stream-alias-2',
    batch: 'Fix the three measured blockers the alias review rejected: a spent source reader ' +
      'aborts the rest of the group where the pin resumes the queue; a command client sourcing ' +
      'a file loses its caller stream, which also makes command-stream-channel.md wrong; and a ' +
      'raw buffer writer inside an alias gains a trailing newline because the group stdout claim ' +
      'falls back to the alias name. Assert each with sabotages in compat/tui-command-streams.sh.',
  },
  {
    key: 'modes-12', obligation: 'TUI-014', from: 'campaign/tui-modes-12-prepush',
    push: 'campaign/tui-modes-12',
    batch: 'Rebase, prove and push the unproven eight-commit fix pass against the modes reject: ' +
      'injected-key consumption, the pane mode stack and its restored surface, switch-mode ' +
      'formats and command templates, server-access identity expansion, tied switch window ' +
      'ordering, the declared footprint, the removed comments and the noattr test.',
  },
  {
    key: 'customize', obligation: 'TUI-014', from: 'campaign/tui-modes-12',
    push: 'campaign/tui-customize',
    batch: 'Build customize-mode against the pin and land suspend-client, TUI-014 clause 2\'s ' +
      'last two entries. fabrico ruled on 2026-09-16 that both are built rather than decided out. ' +
      'The lane resolves the surface question - PaneMode is per-pane but has no navigation at all ' +
      '(selected and offset are rebuilt as 0), while the choose-tree engine has the tree but is ' +
      'per-client - and records the choice in knowledge/designs. Flip customize-mode-open and ' +
      'suspend-client from recorded to asserted, which takes TUI-014 to zero records.',
  },
  {
    key: 'exitcode', obligation: 'TUI-011', from: 'origin/main',
    push: 'campaign/tui-exitcode',
    batch: 'v0.10.0 applied zz\'s own CLI exit-code contract (2 = usage error) to the ' +
      'tmux-compatible command path, so every tmux usage error exits 2 where the pin exits 1 ' +
      '(eight of eight measured). Three asserted cases are red on main. fabrico ruled on ' +
      '2026-09-16 to split the contracts: the tmux path keeps the pin\'s status, zz\'s own verbs ' +
      'and extensions keep 2. The split is per error, not per command, because list-panes carries ' +
      'both a tmux flag surface and zz\'s --json.',
  },
  {
    key: 'capture-12', obligation: 'TUI-017', from: 'campaign/tui-capture-11',
    push: 'campaign/tui-capture-12',
    batch: 'The four must-fix findings that rejected capture-11: compound targets cli:= , =: and ' +
      ':.%2 still failing, erased backgrounds counting toward the -J/-T text extent, SpacerHead ' +
      'skipped at a wide-character wrap boundary, and -C capture losing tabs with no owned record.',
  },
  {
    key: 'capture-residuals', obligation: 'TUI-015', from: 'campaign/tui-capture-11',
    push: 'campaign/tui-capture-residuals',
    batch: 'The four owned records the capture fix pass kept: pane-base-index in the target ' +
      'grammar for lock-session, has-session and list-windows, which lives in CommandEngine ' +
      'outside the resolver excursion; and the indexed-colour-1 class, an engine question about ' +
      'whether zz-terminal can keep the colour class the pin keeps.',
  },
  {
    key: 'zz-knobs', obligation: 'superset', from: 'campaign/tui-customize',
    push: 'campaign/tui-zz-knobs',
    batch: 'The second half of fabrico\'s 2026-09-16 ruling: zz\'s own TUI options and knobs live ' +
      'in the customize-mode tree beside the tmux-compatible ones. Split from the customize lane ' +
      'because it closes no acceptance clause and must not gate the campaign, and because ' +
      'zz-config is not a dependency of zz-tui, zz-daemon or zz-mux and its settings table has no ' +
      'wire path today. The pin-compared rows must stay byte-identical while the zz section exists.',
  },
];

const STAGES = `
2. Every lane is reviewed adversarially by a fresh agent that measures instead of reading. The
reviewer method runs the branch's fixtures three times plus --self-check, writes its own pinned-tmux
probes at the clauses the branch claims, runs the DELTA CORPUS for the touched commands, audits the
evidence directory, and reports expected gate conflicts read-only. A reject on a divergence the
branch honestly recorded with an owner is not a reject.

3. Build zz once per gate and run the proof fixtures the three-dot diff can reach:
compat/tui-pane-geometry.sh, compat/status-row.sh, compat/attached-client.sh,
compat/tui-screen-diff.sh, compat/tui-stock-keys.sh, compat/tui-indicators.sh,
compat/tui-copy-mode.sh, compat/tui-caps.sh, compat/tui-overlays.sh, compat/tui-choosers.sh,
compat/tui-client-commands.sh, compat/tui-command-streams.sh, compat/tui-mouse.sh,
compat/tui-superset.sh, compat/tui-launch-diff.sh and compat/tui-output-backpressure.sh, each with
its --self-check, plus per-crate tests and clippy -D warnings. An intermediate gate runs only what a
rebase can change; the LAST gate in the order runs everything once, including the shared keys and
status scenario sets, and restamps compat/results/summary.md with a full compat/run.sh
--attached-client for CI.

4. Gate order and the held rule. Each branch gets its own gate agent, in ORDER, each pushing main
when green, because one gate agent carrying five branches once stopped partway and left main
untouched. A dependent obligation whose proof is complete but whose dependency is not yet verified
is filled in and held on that dependency at status review; the gate that verifies the dependency
flips it after re-running its fixture. TUI-011 is the last one: it verifies when TUI-014, TUI-015,
TUI-016, TUI-017 and TUI-018 are all verified on main, and the gate that lands the last of them
re-runs compat/tui-client-commands.sh three times plus --self-check, reads its owners tally, and
fills TUI-011's proof block.
`;

async function run() {
  const gates = [];
  for (const key of ORDER) {
    const lane = LANES.find((l) => l.key === key);
    if (!lane) continue;
    gates.push(`gate:${lane.key}`);
    log(`${lane.obligation} ${lane.from} -> ${lane.push}`);
  }
  return { slots: SLOTS, order: ORDER, gates, common: COMMON.length };
}

return run();
