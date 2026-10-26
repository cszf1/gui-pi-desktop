# inst2.sh TAG : install the .deb of TAG (dpkg -i: same version number as before), drop the B1 test bypass
set -e
cd /data/workspace/rel/$1
sudo dpkg -i gui-pi-desktop_*_amd64.deb > /tmp/dpkg.log 2>&1; echo dpkg=$?; tail -2 /tmp/dpkg.log
R=/usr/lib/gui-pi-desktop/resources
(cd $R && sha256sum -c --quiet SHA256SUMS && echo "resources: all match SHA256SUMS")
grep -o '"resources/gui-gate"' /dev/null || true
cmp -s $R/SHA256SUMS resources.SHA256SUMS && echo "resources.SHA256SUMS == release asset"
P=~/.local/share/io.github.cszf1.gui-pi-desktop/pi-agent
rm -f $P/auth.json; ls $P
