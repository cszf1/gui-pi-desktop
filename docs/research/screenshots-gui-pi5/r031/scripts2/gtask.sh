# gtask.sh NAME "TASK" : GUI run (no --run): start the app, type the task, Start; returns at once
. /data/workspace/env.sh
N=$1; R=/data/workspace/runs/r031/$N; rm -rf $R; mkdir -p $R; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
pkill -f "[g]atelab.py"; : > /tmp/gatelab.jsonl
setsid nohup python3 /data/workspace/gatelab.py > /dev/null 2>&1 < /dev/null &
for f in gate agent human; do wc -l < $D/$f.jsonl 2>/dev/null || echo 0; done > $R/offsets; date +%s > $R/t0
bash /data/workspace/r031/gui.sh $BIN > /dev/null
W=$(xdotool search --name "^gui-pi$" | head -1); xdotool windowactivate $W; sleep 1
xdotool mousemove 454 154 click 1; sleep 0.5; xdotool type "$2"; sleep 0.3
xdotool mousemove 52 216 click 1; echo "started $N win=$W"
