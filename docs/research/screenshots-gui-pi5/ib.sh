#!/bin/bash
# usage: ib.sh run <localscript>   | ib.sh cmd '<command>'
export XDG_CONFIG_HOME=/workspace/work/.ibcfg
if [ "$1" = run ]; then
  n=$(basename "$2")
  npx -y instabox@latest exec gui-pi5 --stdin -- "cat > /data/workspace/$n" < "$2" 2>&1 | grep -v '^npm' >/dev/null
  shift 2
  timeout ${T:-110} npx -y instabox@latest exec gui-pi5 -- "bash /data/workspace/$n $*" 2>&1 | grep -v '^npm'
else
  timeout ${T:-110} npx -y instabox@latest exec gui-pi5 -- "$2" 2>&1 | grep -v '^npm'
fi
