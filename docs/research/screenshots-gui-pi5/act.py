import sys, json
for l in open(sys.argv[1]):
    try: e = json.loads(l)
    except Exception: continue
    if e.get("ev") == "action" and (len(sys.argv) < 3 or str(e.get("aid")) == sys.argv[2]):
        for k in ("aid","decision","tier","risk","risk_level","ack","approval","risk_gate","trail","judge","check","reason"):
            print(" ", k, json.dumps(e.get(k), ensure_ascii=False)[:500])
