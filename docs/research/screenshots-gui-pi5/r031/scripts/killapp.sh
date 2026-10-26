# stop every running gui-pi-desktop (deb or AppImage) by its executable, never a shell that mentions the name
for p in $(pgrep -f gui-pi-desktop); do e=$(readlink /proc/$p/exe 2>/dev/null); case "$e" in */gui-pi-desktop|*.AppImage) kill $p 2>/dev/null;; esac; done
sleep 1
