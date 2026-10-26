. /data/workspace/env.sh
R=$1; mkdir -p $R/shots; i=0
while pgrep -f "gui-pi-desktop --run" >/dev/null; do
  w=$(xdotool search --name "Approve this step|A question for you|Your turn|The agent is stuck" 2>/dev/null | head -1)
  if [ -n "$w" ]; then n=$(xdotool getwindowname $w); i=$((i+1)); import -window root -resize 50% "$R/shots/pop$i.png" 2>/dev/null; echo "$(date +%T) $n" >> $R/shots/index.txt; sleep 4; fi
  sleep 0.7
done
