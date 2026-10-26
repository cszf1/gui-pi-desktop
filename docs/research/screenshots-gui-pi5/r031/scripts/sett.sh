# sett.sh : open Settings (button in the main window), move it to 920,0
. /data/workspace/env.sh
W=$(xdotool search --name "^gui-pi$" | head -1); xdotool windowactivate $W; sleep 0.5
xdotool mousemove 844 82 click 1; sleep 3
S=$(xdotool search --name "^Settings$" | head -1); xdotool windowmove $S 960 0; echo settings=$S
xdotool getwindowgeometry $S
