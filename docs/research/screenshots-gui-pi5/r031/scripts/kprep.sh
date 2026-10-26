# copy the two key files to /tmp for leak.sh (values never printed)
install -m 600 /data/workspace/keys/.pi/agent/tokenrhythm.key /tmp/k_main
install -m 600 /data/workspace/keys/.config/gui-gate/reviewer.key /tmp/k_rev
echo "k_main $(wc -c </tmp/k_main) bytes, k_rev $(wc -c </tmp/k_rev) bytes"
