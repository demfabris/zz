from pathlib import Path
import subprocess

folder = Path(__file__).resolve().parent
for iteration in [1, 2]:
    subprocess.run(['python3', str(folder / 'direct-pairs.py'), f'fixed-throttled-{iteration}'], check=True)
    for side in ['candidate', 'main']:
        assert (folder / f'fixed-throttled-{iteration}-{side}.txt').read_text().endswith('exit_status: 0\n')
