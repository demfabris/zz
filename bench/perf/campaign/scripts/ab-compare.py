import json,sys,glob,statistics as st
d=sys.argv[1]
def load(kind):
    out={}
    for p in sorted(glob.glob(f'{d}/ab-{kind}-*.json')):
        for m in json.load(open(p))['metrics']:
            z=m.get('zz') or {}
            if 'median' in z: out.setdefault(m['id'],[]).append((z['median'],m.get('kind'),m.get('verdict')))
    return out
pre,post=load('pre'),load('post')
rows=[]
for k in sorted(set(pre)&set(post)):
    a=[x[0] for x in pre[k]];b=[x[0] for x in post[k]];kind=pre[k][0][1]
    ma,mb=st.median(a),st.median(b)
    if ma==0 and mb==0: continue
    ch=(mb-ma)/ma*100 if ma else float('inf')
    worst_post=[x[2] for x in post[k]]
    rows.append((kind,k,ma,mb,ch,worst_post))
reliable=('instr','bytes','count','mem','threads','footprint')
print('reliable kinds, |change| >= 3%:')
for kind,k,ma,mb,ch,v in rows:
    if kind in reliable and abs(ch)>=3: print(f'  {k:42} {kind:6} pre {ma:12.4f} post {mb:12.4f} {ch:+7.1f}%  post verdicts {v}')
print('timing kinds, |change| >= 15%:')
for kind,k,ma,mb,ch,v in rows:
    if kind not in reliable and abs(ch)>=15: print(f'  {k:42} {kind:6} pre {ma:12.4f} post {mb:12.4f} {ch:+7.1f}%  post verdicts {v}')
fails=sorted({k for k in post for x in post[k] if x[2]=='fail'})
prefails=sorted({k for k in pre for x in pre[k] if x[2]=='fail'})
print('post fails:',fails);print('new vs pre:',sorted(set(fails)-set(prefails)))
