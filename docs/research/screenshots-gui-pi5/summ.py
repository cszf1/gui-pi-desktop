import json, sys, os, collections
R = sys.argv[1]
def rd(n):
    out = []
    try:
        for l in open(os.path.join(R, n)):
            try: out.append(json.loads(l))
            except Exception: pass
    except FileNotFoundError: pass
    return out
g = rd("gate.jsonl")
evs = collections.Counter(e.get("ev") for e in g)
print("gate events:", dict(evs))
for e in g:
    ev = e.get("ev")
    if ev == "action":
        t = e.get("target") or {}
        print(f"  action aid={e.get('aid')} {e.get('tool')} [{t.get('kind')}:{t.get('label','')[:30]}] -> {e.get('decision')} tier={e.get('tier')} risk={e.get('risk_level')} reason={str(e.get('reason'))[:90]}")
    elif ev in ("human_outcome", "human_answer", "denial", "loop_breaker", "dispatch_refused", "person_burden", "frozen", "outcome_unknown", "task_version", "streak_hint", "denial_superseded", "permit_withheld"):
        print("  ", json.dumps({k: v for k, v in e.items() if k not in ("t", "ts")}, ensure_ascii=False)[:260])
    elif ev == "usage":
        pass
    elif ev not in ("obs", "client", "instance", "tree_extra", "read_hint"):
        print("  ", json.dumps(e, ensure_ascii=False)[:200])
u = [e for e in g if e.get("ev") == "usage"]
print("usage events:", len(u), "sum input", sum(e.get("input") or 0 for e in u), "output", sum(e.get("output") or 0 for e in u))
ns = [e for e in g if "next" in str(e.get("ev")) or e.get("tool") == "gate_next_step"]
print("next_step-ish gate events:", len(ns))
for e in rd("human.jsonl"): print("  human:", json.dumps(e)[:300])
gl = rd("gatelab.jsonl"); print("gatelab truth:", [x.get("label") or x.get("ev") for x in gl])
out = open(os.path.join(R, "run.out")).read().splitlines() if os.path.exists(os.path.join(R, "run.out")) else []
kinds = collections.Counter()
for l in out:
    try: kinds[json.loads(l).get("kind")] += 1
    except Exception: pass
print("ui events:", dict(kinds))
for l in out[-6:]: print("  ui:", l[:300])
