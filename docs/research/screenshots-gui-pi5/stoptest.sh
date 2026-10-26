. /data/workspace/env.sh
R=/data/workspace/runs/stop; mkdir -p $R; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
pkill -f "[g]atelab.py"; : > /tmp/gatelab.jsonl
setsid nohup python3 /data/workspace/gatelab.py > /dev/null 2>&1 < /dev/null &
for f in gate agent human; do wc -l < $D/$f.jsonl; done > $R/offsets
setsid nohup /usr/bin/gui-pi-desktop > /tmp/gpd-gui.log 2>&1 < /dev/null &
sleep 7
W=$(xdotool search --name "^gui-pi$" | head -1); xdotool windowactivate $W; xdotool windowmove $W 0 30; sleep 1
xdotool mousemove 454 154 click 1; sleep 0.5
xdotool type "In the GateLab window click Increment ten times, one click per step, reading the counter after every click."
xdotool mousemove 52 216 click 1
echo started $(date +%s)
