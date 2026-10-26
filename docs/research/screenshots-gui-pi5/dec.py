import json, collections, sys
L = []
for l in open(sys.argv[1]):
    try: L.append(json.loads(l))
    except Exception: pass
c = collections.Counter(); ex = {}
for e in L:
    if e.get("ev") == "action":
        d = e.get("decision"); c[d] += 1
        for t in e.get("trail") or []: c["trail:" + t] += 1
        if d not in ex or d in ("review","wait","check","heal","look_light","look_full"):
            ex.setdefault(d, []).append((e.get("session"), e.get("aid"), e.get("tool"), (e.get("target") or {}).get("label"), e.get("tier"), str(e.get("reason"))[:120], e.get("trail")))
    elif e.get("ev") in ("loop_breaker", "denial", "human_outcome", "reset", "next_step", "outcome_unknown", "frozen", "streak_hint", "dispatch_refused"):
        k = e["ev"] + (":" + str(e.get("level") or e.get("outcome") or e.get("option") or "") )
        c[k] += 1
for k, v in sorted(c.items()): print(f"{v:4d} {k}")
for d, xs in ex.items():
    if d in ("review","wait","check","heal","look_light","look_full","stop_loop","deny"):
        for x in xs[:4]: print(d, x)
