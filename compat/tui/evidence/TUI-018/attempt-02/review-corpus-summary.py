import re
from pathlib import Path
root=Path(__file__).parent
expected=set((root/'review-delta-list.txt').read_text().splitlines())
rows={}
for path in sorted(root.glob('review-corpus-*.txt')):
    for line in path.read_text().splitlines():
        if not re.match(r'^\| [^|]+ \| \d+ \|',line):
            continue
        fields=[s.strip() for s in line.strip('|').split('|')]
        name=fields[0]+'.txt'
        if name in expected:
            rows[name]=(fields,path.name)
print(f'Completed {len(rows)}/{len(expected)} rows; {sum(int(v[0][1]) for v in rows.values())} steps')
for name,(fields,path) in sorted(rows.items()):
    if fields[2:]!=['yes','0','yes','yes','yes']:
        print(name,fields[2:],path)
missing=sorted(expected-rows.keys())
if missing:
    print('Next unmeasured:',', '.join(missing[:8]))
