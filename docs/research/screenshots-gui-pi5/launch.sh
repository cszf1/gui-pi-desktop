. /data/workspace/env.sh
pkill -f /usr/bin/gui-pi-desktop; sleep 1
cd /data/workspace
EXTRA="$1"
setsid nohup env $EXTRA /usr/bin/gui-pi-desktop > /tmp/gpd.log 2>&1 < /dev/null &
sleep 8; pgrep -a gui-pi-desktop; cat /tmp/gpd.log | head -30
xdotool search --name "." getwindowname %@ 2>/dev/null | grep -iv "^$" | head
