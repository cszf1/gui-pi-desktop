# sc.sh [APPIMAGE] : start the app headless only for its self-check (deb when no argument), print the check lines
. /data/workspace/env.sh
bash /data/workspace/r031/killapp.sh
if [ -n "$1" ]; then CMD="env APPIMAGE_EXTRACT_AND_RUN=1 $1"; else CMD=/usr/bin/gui-pi-desktop; fi
GUI_PI_HEADLESS=1 timeout 60 $CMD > /tmp/sc.out 2> /tmp/sc.err < /dev/null &
for i in $(seq 1 55); do grep -q "self-check\] bundle" /tmp/sc.err && break; sleep 1; done
grep "self-check" /tmp/sc.err; bash /data/workspace/r031/killapp.sh; true
