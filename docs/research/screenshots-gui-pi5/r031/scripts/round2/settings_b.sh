# settings_b.sh STEP : B5/B6/B10 helpers. shot -> /tmp/s.png of the Settings window
. /data/workspace/env.sh
S=$(xdotool search --name "^Settings$" | head -1)
case "$1" in
 open) bash /data/workspace/r031/gui.sh > /dev/null; bash /data/workspace/r031/sett.sh ;;
 click) xdotool windowactivate $S; sleep 0.3; xdotool mousemove $2 $3 click 1 ;;
 type) xdotool windowactivate $S; xdotool mousemove $2 $3 click --repeat 3 1; sleep 0.2; xdotool key ctrl+a; xdotool type "$4" ;;
 typekey) xdotool windowactivate $S; xdotool mousemove $2 $3 click 1; sleep 0.2; tr -d "\r\n" < /data/workspace/keys/.pi/agent/tokenrhythm.key | xdotool type --file - ;;
 scroll) xdotool windowactivate $S; xdotool mousemove $2 $3; xdotool click --repeat $4 $5 ;;
esac
