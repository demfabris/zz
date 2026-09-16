import json
from pathlib import Path

p = Path(__file__).resolve().parent
text = (p / '15-capture-probe.txt').read_text()
assert 'exit_status: 0' in text
results, _ = json.JSONDecoder().raw_decode(text[text.index('\n{') + 1:])
assert results['tmux'].keys() == results['zz'].keys()
comparisons = []
differences = []
for scene in results['tmux']:
    for mode, expected in results['tmux'][scene].items():
        actual = results['zz'][scene][mode]
        entry = {'scene': scene, 'mode': mode, 'expected': expected, 'actual': actual}
        if expected != actual:
            differences.append(entry)
        in_scope = scene.startswith('target/') or scene == 'resized-erase'
        in_scope |= any(scene.endswith('/' + name) for name in ['erase-line', 'erase-display', 'clear', 'scroll-region']) and mode in ['join', 'trim']
        in_scope |= scene.endswith('/wide-wrap') and mode in ['join', 'escape', 'padding']
        if in_scope:
            comparisons.append(entry)
summary = {'batch_compared': len(comparisons), 'batch_differences': [entry for entry in comparisons if entry['expected'] != entry['actual']], 'all_differences': differences}
(p / 'probe-summary.json').write_text(json.dumps(summary, indent=2) + '\n')
print(f"Batch-scope comparisons: {len(comparisons)}, differences: {len(summary['batch_differences'])}; all probe differences: {len(differences)}")
assert len(comparisons) == 42
assert not summary['batch_differences']
