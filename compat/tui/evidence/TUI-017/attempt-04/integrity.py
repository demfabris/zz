import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

root = Path.cwd()
p = Path(__file__).resolve().parent
env = json.loads((p / 'environment.json').read_text())
main = json.loads((p / 'main-environment.json').read_text())
checks = {}
for name, path, expected in [
    ('candidate_binary', Path(env['binary']), env['binary_sha256']),
    ('main_binary', Path(main['binary']), main['sha256']),
    ('shared_fixture', root / 'compat/tui-client-commands.sh', env['fixture_sha256']),
    ('tmux_pin', root / 'compat/.cache/tmux-src/tmux', env['pin_sha256']),
]:
    actual = hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()
    assert actual == expected, name
    checks[name] = actual
base = env['base']
scenario = Path('compat/scenarios/targets.txt')
assert scenario.read_bytes() == subprocess.check_output(['git', 'show', base + ':' + str(scenario)])
checks['main_target_scenario_unchanged'] = True

def test_body(text):
    start = text.index('    fn session_targets_accept_pane_and_window_ids()')
    end = text.index('\n    #[test]', start)
    return text[start:end]

assert test_body(Path('crates/zz-mux/src/model.rs').read_text()) == test_body(subprocess.check_output(['git', 'show', base + ':crates/zz-mux/src/model.rs'], text=True))
checks['main_target_test_unchanged'] = True
secret_values = [value.encode() for key, value in os.environ.items() if re.search(r'(TOKEN|PASSWORD|SECRET|API_KEY|CREDENTIAL)', key) and len(value) >= 20]
hits = [str(path.relative_to(root)) for path in p.iterdir() if path.is_file() and any(value in path.read_bytes() for value in secret_values)]
assert not hits, 'Credential value matches in ' + ', '.join(hits)
checks['credential_values_checked'] = len(secret_values)
checks['credential_value_matches'] = 0
(p / 'integrity.json').write_text(json.dumps(checks, indent=2) + '\n')
print('Candidate/main binaries, pin and fixture hashes match; main target test and scenario are unchanged; no credential-value matches in fresh evidence')
