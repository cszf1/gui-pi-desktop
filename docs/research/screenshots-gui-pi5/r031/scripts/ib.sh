#!/bin/bash
# local helper: ib.sh up FILE... (to /data/workspace/r031/) | ib.sh run 'cmd' | ib.sh shot NAME (root screenshot -> ../NAME.jpg)
export XDG_CONFIG_HOME=/workspace/work/.ibcfg
IB="npx -y instabox@latest"
case "$1" in
  up) shift; for f in "$@"; do timeout 100 $IB exec gui-pi5 --stdin -- "mkdir -p /data/workspace/r031 && cat > /data/workspace/r031/$(basename $f)" < "$f" 2>&1 | grep -v npm || true; done ;;
  run) timeout ${T:-115} $IB exec gui-pi5 -- "$2" 2>&1 | grep -v "^npm" ;;
  shot) timeout 100 $IB exec gui-pi5 -- ". /data/workspace/env.sh; import -window root ${3:+-crop $3} -quality 75 /tmp/shot.jpg 2>/dev/null; base64 -w0 /tmp/shot.jpg" 2>/dev/null | grep -v "^npm" | base64 -d > /workspace/work/gp5/r031/$2.jpg; ls -la /workspace/work/gp5/r031/$2.jpg | awk '{print $5, $9}' ;;
  get) timeout 100 $IB exec gui-pi5 -- "base64 -w0 $2" 2>/dev/null | grep -v "^npm" | base64 -d > "$3"; ls -la "$3" | awk '{print $5, $9}' ;;
esac
