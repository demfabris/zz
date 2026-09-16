import json
from pathlib import Path
import shlex
import subprocess

root = Path.cwd()
evidence = root / 'compat/tui/evidence/TUI-014/attempt-07-customize'
corpus = json.loads((evidence / 'corpus-summary.json').read_text())
assert not corpus['unrun']
source_revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
path = root / 'compat/tui/campaign.json'
campaign = json.loads(path.read_text())
item = next(item for item in campaign['items'] if item['id'] == 'TUI-014')
item['status'] = 'review'
item['proof']['revision'] = source_revision
item['proof']['environment'] = item['proof']['environment'].replace('one timing-sensitive test, formatting and documentation', 'one timing-sensitive test, fixture inventory totals, formatting and documentation')
item['proof']['commands'] = [command for command in item['proof']['commands'] if 'delta corpus' not in command]
item['proof']['commands'].append(shlex.join(corpus['command']) + f": exit {corpus['exit']}, {corpus['completed']} rows, {corpus['steps']} steps, zero unrun, {len(corpus['failed'])} failed after retry; see corpus-summary.json")
item['proof']['commands'].append('HOME=/tmp/zz-emptyhome XDG_CONFIG_HOME=/tmp/zz-emptyhome/config compat/check.sh with explicit toolchain homes and capped-bin PATH: exit 0 (compat-check-scrubbed)')
item['proof']['artifacts'] = [
    'compat/tui/evidence/TUI-014/attempt-06/notes.md',
    'compat/tui/evidence/TUI-014/attempt-07/notes.md',
    *[str(path.relative_to(root)) for path in sorted(evidence.rglob('*')) if path.is_file()],
]
item['evidence_note'] = (
    'Customize lane, measured 2026-09-16, based on modes b2c00bd2. '
    'customize-mode-open and suspend-client flip from recorded to asserted, with one-sided expansion and actual process-suspension sabotages. '
    'The tree is per pane and shares the existing mode stack. Default roots match the pin without a zz-row mask; the sibling adds its explicit reveal section later. '
    'The final rebuilt v104 verifier exits 0: client commands 141 asserted/28 recorded, none owned by TUI-014, unattributed=0; choosers 78 asserted/0 recorded. '
    'Across four full client rosters both assigned cases pass every time; the second roster retains a seconds-clock mismatch. Earlier chooser/verifier zoom failures remain preserved. '
    'Focused edit/stop/resume checks pass 12/12, and all eleven added CLI flag diagnostics match stdout, stderr and status. Both client-command self-check runs pass. '
    'Copy-mode, screen-diff, overlays and their self-checks pass; attached-client passes and has no self-check entry point. '
    'The full four-crate run retains one delayed-job daemon failure that passes alone; all other targets pass. Clippy, formatting, the scrubbed-home compatibility gate, wire check and knowledge validation pass. '
    f"The delta corpus completes {corpus['completed']} rows and {corpus['steps']} steps, zero unrun, with {len(corpus['failed'])} failures after retry; raw first and retry logs are retained. "
    'Corpus and broader regressions use the retained initial v104 binary; final verifier/focused checks use the rebased build. '
    'Key-binding editing, reset/unset and tagged bulk mutations, array insertion, help, mouse and -k/-Z remain unimplemented. Built-in SSH supplies no tty and takes the suspend no-op path; no live remote proof is claimed. '
    'The evidence records limits and failures rather than claiming exhaustive customize parity. Status is review; the gate owns verification.'
)
path.write_text(json.dumps(campaign, indent=2) + '\n')
notes = evidence / 'notes.md'
notes.write_text(notes.read_text() + '\n## Completed delta measurement\n\n' +
    f"The full selected corpus completed {corpus['completed']} rows and {corpus['steps']} steps with zero unrun rows, exit {corpus['exit']}. " +
    f"Failed after the isolated retry: {', '.join(corpus['failed']) or 'none'}. " +
    f"Recovered on retry: {', '.join(corpus['recovered']) or 'none'}. " +
    'Both first and final logs are in corpus-logs.tar.gz; corpus-log-manifest.json records their SHA-256 hashes. The archive was verified against every source byte before removing the temporary copied directory. The harness footer can say Nothing failed on the first pass when none recovered; corpus-summary.json uses its actual per-row results and retry warnings instead.\n')
subprocess.run(['python3', 'compat/tui/tracker.py', 'write-report'], check=True)
