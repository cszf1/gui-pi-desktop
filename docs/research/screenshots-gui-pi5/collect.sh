# collect.sh NAME : copy this run's slice of the logs and summarize gate decisions
N=$1; R=/data/workspace/runs/$N; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
running=$(pgrep -f "gui-pi-desktop --run" >/dev/null && echo yes || echo no); echo "running=$running elapsed=$(( $(date +%s) - $(cat $R/t0) ))s"
i=0; for f in gate agent human; do i=$((i+1)); off=$(sed -n ${i}p $R/offsets); tail -n +$((off+1)) $D/$f.jsonl > $R/$f.jsonl 2>/dev/null; done
cp /tmp/gatelab.jsonl $R/gatelab.jsonl
python3 /data/workspace/summ.py $R
