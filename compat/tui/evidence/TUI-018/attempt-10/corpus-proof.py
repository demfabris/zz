from pathlib import Path
import hashlib,json,re
p=Path('compat/tui/evidence/TUI-018/attempt-10')
selected=(p/'delta-final-selection.txt').read_text().splitlines()
finished={}
shards=[]
for i in range(1,8):
 f=p/f'corrected-corpus-{i}.txt';s=f.read_text();status=re.search(r'^exit_status=(\d+)$',s,re.M)
 shards.append({'artifact':str(f),'exit_status':int(status[1]) if status else None})
 for m in re.finditer(r'^([^\n:]+): (\d+) step\(s\), (.*)$',s,re.M):finished[m[1]+'.txt']=i
missing=sorted(set(selected)-set(finished))
if missing or any(x['exit_status'] is None for x in shards):
 print(json.dumps({'completed':len(finished),'selected':len(selected),'missing':len(missing),'shards':shards},indent=2));raise SystemExit(2)
known={x['scenario']:x for x in json.loads(Path('compat/tmux-gaps.json').read_text())['known_differentials']}
rows=[];failures=[]
for row in selected:
 f=Path('compat/results')/row.replace('.txt','.log');raw=f.read_bytes();s=raw.decode(errors='replace');summary=re.findall(r'^SUMMARY .*$',s,re.M)[-1]
 values={k:int(v) for k,v in re.findall(r'(\w+)=(\d+)',summary)}
 counts={k:values[k+'_divergences'] for k in ['topo','geo','fmt','out','warn']}
 classification='zero-divergence'
 if row in known and all(counts[k]==known[row][k] for k in counts):classification='registered-gap'
 elif any(counts.values()):classification='unclassified';failures.append(row)
 rows.append({'row':row,'shard':finished[row],'summary':summary,'sha256':hashlib.sha256(raw).hexdigest(),'scenario_sha256':hashlib.sha256((Path('compat/scenarios')/row).read_bytes()).hexdigest(),'classification':classification,'steps':values['steps']})
comparisons=[]
for row in failures:
 main=p/'main-comparisons'/row
 if not main.exists():continue
 candidate=Path('compat/results')/row.replace('.txt','.log');destination=p/'candidate-comparisons'/row;destination.parent.mkdir(parents=True,exist_ok=True);destination.write_bytes(candidate.read_bytes())
 normalize=lambda s:'\n'.join(line for line in s.splitlines() if not line.startswith('# Source:'))
 identical=normalize(candidate.read_text())==normalize(main.read_text())
 comparisons.append({'row':row,'candidate':str(destination),'main':str(main),'identical_except_source_header':identical})
 if identical:
  next(x for x in rows if x['row']==row)['classification']='inherited'
 elif row=='smoke/source-replay-diagnostics.txt':
  before='request-rc:124,stderr:_,events:0:end:_'
  after='request-rc:124,stderr:_,events:'
  same_timeout=normalize(candidate.read_text().replace(before,after))==normalize(main.read_text())
  comparisons[-1]['same_timeout_except_additional_empty_guard']=same_timeout
  comparisons[-1]['retained_diff']=str(p/'source-replay-main-comparison.diff')
  if same_timeout:
   next(x for x in rows if x['row']==row)['classification']='inherited'
 elif row=='smoke/status-background-jobs.txt':
  initial=p/'initial-corpus-failures'/row
  same_initial=normalize(initial.read_text())==normalize(main.read_text())
  comparisons[-1]['initial_candidate_identical_to_main']=same_initial
  comparisons[-1]['retained_diff']=str(p/'status-background-main-comparison.diff')
  if same_initial:
   next(x for x in rows if x['row']==row)['classification']='inherited'
data={'production_revision':'5e601c02','binary_sha256':hashlib.sha256(Path('target/debug/zz-alias5-corrected').read_bytes()).hexdigest(),'selection':str(p/'delta-final-selection.txt'),'selected_rows':len(selected),'completed_unique_rows':len(rows),'steps':sum(x['steps'] for x in rows),'counts':{kind:sum(x['classification']==kind for x in rows) for kind in ['zero-divergence','registered-gap','inherited','unclassified']},'shards':shards,'rebalance':str(p/'corpus-rebalance.json'),'comparisons':comparisons,'rows':rows,'transcript_policy':'All row summaries and hashes retained; full transcripts copied only for targeted failure comparisons, with no environment dumps.'}
(p/'corpus-proof.json').write_text(json.dumps(data,indent=2)+'\n');print(json.dumps({k:v for k,v in data.items() if k not in ['rows','comparisons']},indent=2))
