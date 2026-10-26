. /data/workspace/env.sh
# args: xdotool command string(s) separated by ';;'
IFS=';;' read -ra parts <<< "$1"
for p in "${parts[@]}"; do [ -n "$p" ] && eval "xdotool $p"; sleep 0.4; done
sleep ${2:-2}
import -window root -resize 50% /tmp/s.png 2>/dev/null; base64 -w0 /tmp/s.png
