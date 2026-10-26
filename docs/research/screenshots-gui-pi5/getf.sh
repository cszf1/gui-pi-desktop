#!/bin/bash
# getf.sh REMOTE LOCAL
cd /workspace/work/gp5; XDG_CONFIG_HOME=/workspace/work/.ibcfg npx -y instabox@latest exec gui-pi5 -- "base64 -w0 $1" 2>/dev/null | grep -v '^npm' | tr -d '\n' | base64 -d > $2; file $2
