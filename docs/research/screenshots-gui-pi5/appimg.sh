. /data/workspace/env.sh
pkill -f "[g]ui-pi-desktop"; sleep 1
A=/data/workspace/rel/build-f2b6d6b/gui-pi-desktop_0.3.0_amd64.AppImage; chmod +x $A
cd /data/workspace/rel/build-f2b6d6b && sha256sum -c SHA256SUMS 2>&1 | grep AppImage
setsid nohup env APPIMAGE_EXTRACT_AND_RUN=1 $A > /tmp/gpd-appimage.log 2>&1 < /dev/null &
sleep 20; pgrep -af "gui-pi-desktop" | cut -c1-120 | head -3
W=$(xdotool search --name "^gui-pi$" | head -1); echo win=$W; xdotool windowmove $W 0 30; sleep 1
xdotool mousemove 844 82 click 1; sleep 6
import -window root -resize 50% /data/workspace/runs/appimage.png; head -5 /tmp/gpd-appimage.log
