. /data/workspace/env.sh
R=$1; mkdir -p $R/shots; i=0
while pgrep -f "gui-pi-desktop --run" >/dev/null; do
  w=$(timeout 3 xdotool search --name "Approve this step|A question for you|Your turn|The agent is stuck" 2>/dev/null | head -1)
  if [ -n "$w" ]; then n=$(timeout 3 xdotool getwindowname $w); i=$((i+1)); sleep 1.2
    eval $(timeout 3 xwininfo -id $w | awk -F: '/Absolute upper-left X/{print "X="$2+0} /Absolute upper-left Y/{print "Y="$2+0} /Width/{print "WIDTH="$2+0} /Height/{print "HEIGHT="$2+0}')
    # root capture cropped to the popup (an import of the popup window itself can hang X if it closes mid-capture)
    timeout 8 import -window root -crop ${WIDTH}x${HEIGHT}+${X}+${Y} "$R/shots/popwin$i.png" 2>/dev/null
    timeout 8 import -window root -resize 50% "$R/shots/pop$i.png" 2>/dev/null
    echo "$(date +%T) $n ${WIDTH}x${HEIGHT}+${X}+${Y}" >> $R/shots/index.txt; sleep 4; fi
  sleep 0.7
done
