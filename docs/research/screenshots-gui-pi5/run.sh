# run.sh NAME RULES_JSON TASK [HINT]  -> starts a headless --run task in background
. /data/workspace/env.sh
N=$1; RULES=$2; TASK=$3; HINT=$4
R=/data/workspace/runs/$N; mkdir -p $R; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
pkill -f /usr/bin/gui-pi-desktop; pkill -f gatelab.py; sleep 1
mkdir -p ~/Documents; echo "lab note" > ~/Documents/lab-note.txt
echo "$RULES" > $R/rules.json
: > /tmp/gatelab.jsonl
GATELAB_HINT="$HINT" setsid nohup python3 /data/workspace/gatelab.py > $R/gatelab.out 2>&1 < /dev/null &
sleep 2
for f in gate agent human; do wc -l < $D/$f.jsonl 2>/dev/null || echo 0; done > $R/offsets
echo "$TASK" > $R/task.txt; date +%s > $R/t0
GUI_PI_HUMAN_AUTO=$R/rules.json setsid nohup timeout ${TMO:-420} /usr/bin/gui-pi-desktop --run "$TASK" > $R/run.out 2> $R/run.err < /dev/null &
sleep 1; setsid nohup bash /data/workspace/watch.sh $R > /dev/null 2>&1 < /dev/null &
sleep 1; pgrep -f "gui-pi-desktop --run" >/dev/null && echo "started $N"
