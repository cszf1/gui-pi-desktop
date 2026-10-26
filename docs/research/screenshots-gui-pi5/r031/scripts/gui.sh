# gui.sh [APPIMAGE] : start the app with its window at 0,30 (deb when no argument)
. /data/workspace/env.sh
bash /data/workspace/r031/killapp.sh
if [ -n "$1" ]; then CMD="env APPIMAGE_EXTRACT_AND_RUN=1 $1"; else CMD=/usr/bin/gui-pi-desktop; fi
setsid nohup $CMD > /tmp/gpd-gui.log 2>&1 < /dev/null &
for i in $(seq 1 30); do W=$(xdotool search --name "^gui-pi$" 2>/dev/null | head -1); [ -n "$W" ] && break; sleep 1; done
sleep 2; xdotool windowactivate $W; xdotool windowmove $W 0 30; echo win=$W
