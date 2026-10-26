# gcollect.sh NAME : slice logs of a GUI run, summarize, list processes
N=$1; R=/data/workspace/runs/r031/$N; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
i=0; for f in gate agent human; do i=$((i+1)); off=$(sed -n ${i}p $R/offsets); tail -n +$((off+1)) $D/$f.jsonl > $R/$f.jsonl 2>/dev/null; done
cp /tmp/gatelab.jsonl $R/gatelab.jsonl; python3 /data/workspace/summ.py $R 2>&1 | grep -v "^  ui:"
echo "--- client_tool_unmarked: $(grep -c client_tool_unmarked $R/gate.jsonl)"
