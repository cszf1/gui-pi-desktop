# gate 0cae275 tests against the bundled gui-gate binary (fake_driver), in the background
rm -rf /data/workspace/gg3; mkdir -p /data/workspace/gg3; cd /data/workspace/gg3; tar xzf /data/workspace/r031/ggtests3.tgz
export GUI_GATE_BIN=/usr/lib/gui-pi-desktop/resources/gui-gate
sha256sum $GUI_GATE_BIN > /data/workspace/e2e3.bin
setsid nohup python3 -m unittest tests.test_v04 tests.test_actions_e2e tests.test_remaining tests.test_security_e2e > /data/workspace/e2e3.log 2>&1 < /dev/null &
echo started
