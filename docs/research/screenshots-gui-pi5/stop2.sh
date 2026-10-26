. /data/workspace/env.sh
R=/data/workspace/runs/stop; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
for f in gate agent human; do wc -l < $D/$f.jsonl; done > $R/offsets2
: > /tmp/gatelab.jsonl
W=$(xdotool search --name "^gui-pi$" | head -1); xdotool windowactivate $W; sleep 0.5
xdotool mousemove 454 154 click 1; xdotool key ctrl+a; xdotool type "In the GateLab window click Increment ten times, one click per step, reading the counter after every click."
xdotool mousemove 52 216 click 1; t0=$(date +%s)
for i in $(seq 1 85); do n=$(grep -c '"Increment"' /tmp/gatelab.jsonl); [ "$n" -ge 2 ] && break; sleep 1; done
ps -eo pid,pgid,cmd | grep -E "[c]li.js|[g]ui-gate --human|[c]ua-driver mcp" | cut -c1-120 > $R/procs2_before.txt
c1=$(grep -c '"Increment"' /tmp/gatelab.jsonl); xdotool mousemove 122 216 click 1; ts=$(date +%s)
echo "stopped after $((ts-t0))s with clicks=$c1"; cat $R/procs2_before.txt
sleep 10; c2=$(grep -c '"Increment"' /tmp/gatelab.jsonl); echo "clicks 10s after stop=$c2"
echo "procs after:"; ps -eo pid,pgid,cmd | grep -E "[c]li.js|[g]ui-gate --human|[c]ua-driver mcp" | cut -c1-120
