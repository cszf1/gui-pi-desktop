# wait2.sh NAME [MAXS] : wait for the run to end (<=110 s per call), then collect + summarize
N=$1; R=/data/workspace/runs/r031/$N; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
for i in $(seq 1 ${2:-100}); do [ -f $R/exit ] && break; sleep 1; done
[ -f $R/exit ] || { echo "still running after $(( $(date +%s) - $(cat $R/t0) ))s"; exit 0; }
echo "exit=$(cat $R/exit) elapsed=$(( $(stat -c %Y $R/exit) - $(cat $R/t0) ))s"
i=0; for f in gate agent human; do i=$((i+1)); off=$(sed -n ${i}p $R/offsets); tail -n +$((off+1)) $D/$f.jsonl > $R/$f.jsonl 2>/dev/null; done
cp /tmp/gatelab.jsonl $R/gatelab.jsonl
python3 /data/workspace/summ.py $R
echo "--- client_tool_unmarked: $(grep -c client_tool_unmarked $R/gate.jsonl)"
