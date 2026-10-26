# grep for real key values in given dirs; prints only file names and counts, never the values
. /data/workspace/env.sh
dirs="${@:-$HOME/.config/io.github.cszf1.gui-pi-desktop $HOME/.local/share/io.github.cszf1.gui-pi-desktop $HOME/.cache/io.github.cszf1.gui-pi-desktop $XDG_RUNTIME_DIR/gui-pi-desktop /tmp/gpd.log /tmp/gpd-*.log}"
for k in /tmp/k_main /tmp/k_rev; do
  v=$(cat $k); part=${v:8:20}
  echo "[$k] full-value hits:"; grep -rlaF -- "$v" $dirs 2>/dev/null | grep -v '/tmp/k_' || echo "  none"
  echo "[$k] 20-char-fragment hits:"; grep -rlaF -- "$part" $dirs 2>/dev/null | grep -v '/tmp/k_' || echo "  none"
done
echo "[redaction markers]"; grep -rhoa -- '\*\*\*\|\[redacted\]' $dirs 2>/dev/null | sort | uniq -c
