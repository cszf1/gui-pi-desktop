. /data/workspace/env.sh
R=/data/workspace/runs/stop
ps -eo pid,pgid,cmd | grep -E "[p]i-coding-agent|[g]ui-gate|[c]ua-driver" | cut -c1-150 > $R/procs_before.txt; cat $R/procs_before.txt
grep -c '"label": "Increment"' /tmp/gatelab.jsonl > $R/clicks_at_stop
xdotool mousemove 122 216 click 1; date +%s > $R/t_stop; sleep 4
echo "--- after stop:"; ps -eo pid,pgid,cmd | grep -E "[p]i-coding-agent|[g]ui-gate|[c]ua-driver" | cut -c1-150
echo "clicks at stop: $(cat $R/clicks_at_stop)"
