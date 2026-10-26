cd /data/workspace/gg
export GUI_GATE_BIN=/usr/lib/gui-pi-desktop/resources/gui-gate
setsid nohup python3 -m unittest tests.test_actions_e2e -v > /data/workspace/e2e_actions.log 2>&1 < /dev/null &
echo started
