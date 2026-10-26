# run2.sh NAME RULES_JSON TASK  -> a headless --run task in the background (BIN=AppImage path to use an AppImage)
. /data/workspace/env.sh
N=$1; RULES=$2; TASK=$3
R=/data/workspace/runs/r031/$N; rm -rf $R; mkdir -p $R; D=~/.local/share/io.github.cszf1.gui-pi-desktop/logs
bash /data/workspace/r031/killapp.sh; pkill -f "[g]atelab.py"
mkdir -p ~/Documents; echo "lab note" > ~/Documents/lab-note.txt
echo "$RULES" > $R/rules.json
: > /tmp/gatelab.jsonl
setsid nohup python3 /data/workspace/gatelab.py > $R/gatelab.out 2>&1 < /dev/null &
sleep 2
for f in gate agent human; do wc -l < $D/$f.jsonl 2>/dev/null || echo 0; done > $R/offsets
echo "$TASK" > $R/task.txt; date +%s > $R/t0
if [ -n "$BIN" ]; then CMD="env APPIMAGE_EXTRACT_AND_RUN=1 $BIN"; else CMD=/usr/bin/gui-pi-desktop; fi
( GUI_PI_HUMAN_AUTO=$R/rules.json timeout ${TMO:-420} $CMD --run "$TASK" > $R/run.out 2> $R/run.err < /dev/null; echo $? > $R/exit ) > /dev/null 2>&1 < /dev/null &
disown
sleep 2; setsid nohup bash /data/workspace/watch.sh $R > /dev/null 2>&1 < /dev/null &
sleep 1; pgrep -f "\-\-run" >/dev/null && echo "started $N"
