import subprocess,sys
from pathlib import Path
rows=Path('compat/tui/evidence/TUI-018/attempt-02/review-delta-list.txt').read_text().splitlines()
start=int(sys.argv[1]); count=int(sys.argv[2]) if len(sys.argv)>2 else 8
selected=rows[start:start+count]
print(f'rows {start+1}-{start+len(selected)} of {len(rows)}',flush=True)
raise SystemExit(subprocess.call(['/tmp/zz018-review-run',f'corpus-{start+1:03d}-{start+len(selected):03d}','compat/run.sh','--strict-geometry',*selected]))
