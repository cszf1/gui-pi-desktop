K=$(cat /data/workspace/keys/.pi/agent/tokenrhythm.key | tr -d '\n')
curl -s -m 30 https://tokenrhythm.studio/v1/models -H "Authorization: Bearer $K" | python3 -c 'import sys,json; d=json.load(sys.stdin); print([m["id"] for m in d.get("data",[]) if "qwen" in m["id"].lower()][:40])' 2>&1 | head -5
curl -s -m 60 https://tokenrhythm.studio/v1/chat/completions -H "Authorization: Bearer $K" -H 'Content-Type: application/json' -d '{"model":"qwen3.8-flash","messages":[{"role":"user","content":"Reply OK"}],"max_tokens":20}' | sed "s/$K/***/g" | head -c 600; echo
