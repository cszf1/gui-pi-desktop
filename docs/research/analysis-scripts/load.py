import json,glob,collections,math,random
def sessions():
    out=[]
    for f in sorted(glob.glob('/workspace/work/gui-gate/results/osworld-2026-10-21*/gate/*.jsonl')):
        if f.endswith('.fixture.jsonl'): continue
        evs=[]
        for l in open(f):
            try: evs.append(json.loads(l))
            except: pass
        retro={e['aid'] for e in evs if e.get('ev')=='retro_repeat'}
        acts=[e for e in evs if e.get('ev')=='action' and e.get('executed')]
        for a in acts:
            a['y']=0 if (a.get('is_error') or a.get('stale_error') or a['aid'] in retro) else 1
        out.append((f,evs,acts))
    return out
