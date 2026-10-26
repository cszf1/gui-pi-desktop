# stop3.sh : GUI task, Stop after 2 Increment clicks; check reset, processes, no actions after; then a new task = new processes, Stop again
. /data/workspace/env.sh
R=/data/workspace/runs/r031/stop; rm -rf $R; mkdir -p $R; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
pr() { ps -eo pid,pgid,comm,args | awk '$3=="pi" || /resources\/gui-gate|cua-driver mcp/' | grep -v awk | cut -c1-110; }
pkill -f "[g]atelab.py"; : > /tmp/gatelab.jsonl
setsid nohup python3 /data/workspace/gatelab.py > /dev/null 2>&1 < /dev/null &
bash /data/workspace/r031/gui.sh > /dev/null
TASK="In the GateLab window click Increment ten times, one click per step, reading the counter after every click."
for k in 1 2; do
  for f in gate agent human; do wc -l < $D/$f.jsonl; done > $R/off$k; : > /tmp/gatelab.jsonl
  W=$(xdotool search --name "^gui-pi$" | head -1); xdotool windowactivate $W; sleep 0.5
  xdotool mousemove 454 154 click 1; xdotool key ctrl+a; xdotool type "$TASK"; xdotool mousemove 52 216 click 1; t0=$(date +%s)
  for i in $(seq 1 100); do n=$(grep -c '"Increment"' /tmp/gatelab.jsonl); [ "$n" -ge 2 ] && break; sleep 1; done
  pr > $R/procs_before$k.txt
  c1=$(grep -c '"Increment"' /tmp/gatelab.jsonl); xdotool mousemove 122 216 click 1; ts=$(date +%s)
  sleep 10; c2=$(grep -c '"Increment"' /tmp/gatelab.jsonl); pr > $R/procs_after$k.txt
  i=0; for f in gate agent human; do i=$((i+1)); off=$(sed -n ${i}p $R/off$k); tail -n +$((off+1)) $D/$f.jsonl > $R/$f.$k.jsonl; done
  echo "== task $k: stop after $((ts-t0))s, clicks at stop=$c1, 10 s later=$c2"
  echo "procs before:"; cat $R/procs_before$k.txt; echo "procs after: $(wc -l < $R/procs_after$k.txt) lines"; cat $R/procs_after$k.txt
  python3 - $R/gate.$k.jsonl <<'PY'
import json,sys
ev=[json.loads(l) for l in open(sys.argv[1]) if l.strip()]
ri=[i for i,e in enumerate(ev) if e.get("ev")=="reset"]
print("reset events:", len(ri), "last ev:", ev[-1].get("ev") if ev else None, "| reset:", json.dumps(ev[ri[-1]])[:200] if ri else "-")
print("actions after reset:", sum(1 for e in ev[(ri[-1] if ri else len(ev)):] if e.get("ev")=="action"), "| client_tool_unmarked:", sum(1 for e in ev if e.get("ev")=="client_tool_unmarked"))
PY
  grep -h human_summary $R/human.$k.jsonl | cut -c1-160
done
