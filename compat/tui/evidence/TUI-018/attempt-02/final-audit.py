import json, subprocess
from pathlib import Path
base='7e7cb1ee25122a10e7f1d8815241422eb00c4199'
p='compat/tui/campaign.json'
a=json.loads(Path(p).read_text()); b=json.loads(subprocess.check_output(['git','show',base+':'+p]))
def record(x):
 if isinstance(x,dict):
  if x.get('id')=='TUI-018': return x
  for v in x.values():
   r=record(v)
   if r is not None: return r
 elif isinstance(x,list):
  for v in x:
   r=record(v)
   if r is not None:return r
r=record(a)
assert r['status']=='review' and r['proof'] is None
for d in (r,record(b)):
 for k in ('status','evidence_note','next_action','sources'):d.pop(k,None)
assert a==b
print('Only the four permitted TUI-018 fields differ from the fixed base; status review; proof untouched.',flush=True)
mergebase=subprocess.check_output(['git','merge-base','origin/main','HEAD'],text=True).strip()
assert mergebase==base
print('Current origin/main:',subprocess.check_output(['git','rev-parse','origin/main'],text=True).strip(),flush=True)
print('Three-dot diff merge base remains:',mergebase,flush=True)
e=Path('compat/tui/evidence/TUI-018/attempt-02')
notes=(e/'notes.md').read_text()
for f in e.iterdir():
 assert f.name=='notes.md' or f.name in notes, f.name
 assert f.suffix!='.log',f
print('Every evidence file is named in notes.md; no .log files.',flush=True)
r=subprocess.run(['git','check-ignore','-v',str(e)])
assert r.returncode==1
print('git check-ignore -v exit=1: evidence is not ignored.',flush=True)
subprocess.run(['python3','compat/tui/tracker.py','check'],check=True)
head=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert (e/'final-remote-tip.txt').read_text().split()[0]==head
print('Remote branch matches local tip:',head,flush=True)
subprocess.run(['git','status','--short'],check=True)
