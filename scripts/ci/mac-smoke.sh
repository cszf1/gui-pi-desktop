#!/usr/bin/env bash
# macOS CI smoke test of the built .dmg on the runner's desktop: install the .app from the .dmg, check signatures
# (the nested CuaDriver.app must keep Cua's signature; gui-pi itself is unsigned), start it, read its self-check and
# bundle-integrity lines, take screenshots; the test-hooks build shows the four person popups through the real
# ask-a-person IPC (--demo-popups) and checks each answer (one is left to time out = no answer). The bundled
# gui-gate then runs gui-gate's fake_driver end-to-end suite.
# usage: scripts/ci/mac-smoke.sh <dmg> <out dir> normal|testhooks <gui-gate checkout>
set -uo pipefail
dmg=$1; out=$2; variant=$3; gatesrc=$4
mkdir -p "$out"
fails=()
log() { echo "$*" | tee -a "$out/summary.txt"; }

mnt=$(mktemp -d)
hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$dmg" >/dev/null || { echo "cannot mount $dmg"; exit 1; }
mkdir -p "$HOME/Applications"
rm -rf "$HOME/Applications/gui-pi-desktop.app"
ditto "$mnt/gui-pi-desktop.app" "$HOME/Applications/gui-pi-desktop.app"
hdiutil detach "$mnt" >/dev/null
app="$HOME/Applications/gui-pi-desktop.app"
exe="$app/Contents/MacOS/gui-pi-desktop"
res="$app/Contents/Resources/resources"
log "machine: $(sw_vers -productVersion) $(uname -m)"
for f in "$exe" "$res/node/bin/node" "$res/gui-gate" "$res/CuaDriver.app/Contents/MacOS/cua-driver"; do
  [ -e "$f" ] || fails+=("missing: $f")
  log "$(basename "$f"): $(lipo -archs "$f" 2>/dev/null)"
done
codesign -dv "$app" > "$out/codesign-app.txt" 2>&1; log "gui-pi-desktop.app signature: $(grep -E 'Signature|flags' "$out/codesign-app.txt" | tr '\n' ' ')"
if codesign --verify --deep --strict --verbose=2 "$res/CuaDriver.app" > "$out/codesign-cuadriver.txt" 2>&1; then
  log "CuaDriver.app (nested): signature valid ($(codesign -dv "$res/CuaDriver.app" 2>&1 | grep -E '^Authority=' | head -1))"
else
  fails+=("the nested CuaDriver.app signature is no longer valid"); cat "$out/codesign-cuadriver.txt"
fi
spctl -a -vv "$app" > "$out/gatekeeper.txt" 2>&1; log "Gatekeeper (spctl) on gui-pi-desktop.app: $(head -1 "$out/gatekeeper.txt")"
"$res/CuaDriver.app/Contents/MacOS/cua-driver" permissions status --json > "$out/cua-permissions.json" 2>&1
log "cua-driver permissions status: $(head -c 400 "$out/cua-permissions.json" | tr '\n' ' ')"

shot() { if screencapture -x "$out/$1.png" 2>>"$out/screencapture.err"; then log "screenshot $1.png"; else fails+=("screencapture $1"); fi; }
waitfor() {  # file pattern seconds
  local end=$((SECONDS + $3))
  while [ $SECONDS -lt $end ]; do grep -qF "$2" "$1" 2>/dev/null && return 0; sleep 0.25; done
  return 1
}
so="$out/app.stdout.txt"; se="$out/app.stderr.txt"
if [ "$variant" = testhooks ]; then
  echo '{"delay_ms": 7000, "approve": ["yes"], "answer": ["Shuffle labels"], "takeover": ["yes"], "ask_help": ["timeout"]}' > "$out/auto.json"
  GUI_PI_HUMAN_AUTO="$out/auto.json" GUI_PI_DEMO_LEAD_MS=5000 "$exe" --demo-popups 15 --show-settings > "$so" 2> "$se" &
  pid=$!
  if waitfor "$se" "[self-check] bundle" 120; then sleep 3; shot 01-main-and-settings; else fails+=("no self-check output"); fi
  n=1
  for t in approve clarify takeover help; do
    n=$((n + 1))
    if waitfor "$so" "\"type\":\"$t\"" 90; then sleep 2.5; shot "0$n-popup-$t"; else fails+=("popup $t never asked"); fi
  done
  for _ in $(seq 1 360); do kill -0 $pid 2>/dev/null || break; sleep 0.25; done
  kill -0 $pid 2>/dev/null && { fails+=("demo did not finish"); kill -9 $pid; }
  wait $pid; log "app exit $?"
  python3 - "$so" >> "$out/summary.txt" <<'EOF' || fails+=("popup replies wrong (see summary.txt)")
import json, sys
want = {"approve": ("accept", "allow"), "answer": ("accept", "Shuffle labels"), "takeover": ("accept", "true"), "ask_help": ("decline", "timed_out")}
got = {}
for line in open(sys.argv[1]):
    try:
        m = json.loads(line)
    except Exception:
        continue
    if m.get("kind") == "demo_reply":
        got[m["gate_kind"]] = m.get("reply")
bad = 0
for k, (action, word) in want.items():
    r = got.get(k)
    flat = json.dumps(r)
    ok = isinstance(r, dict) and r.get("action") == action and word.lower() in flat.lower()
    print(f"reply {k}: {'ok' if ok else 'WRONG'} {flat}")
    bad += not ok
sys.exit(1 if bad else 0)
EOF
else
  "$exe" > /dev/null 2>&1 &
  pid=$!; sleep 15; shot 01-main-window; kill $pid 2>/dev/null; sleep 2
  "$exe" --run "smoke test" > "$so" 2> "$se"; rc=$?
  log "app --run exit $rc (3 = refused to start: no models configured)"
  [ $rc -eq 3 ] || fails+=("--run exit $rc, expected 3")
fi
echo "--- stderr"; head -60 "$se"; echo "--- stdout"; head -40 "$so"
grep -q "bundled files match the release's SHA256SUMS" "$se" || fails+=("bundle integrity not confirmed")
for id in macos accessibility screen keychain; do
  line=$(grep -m1 "^\[self-check\] $id:" "$se")
  log "${line:-[self-check] $id: MISSING}"
  [ -n "$line" ] || fails+=("self-check $id missing")
done

if [ "$variant" = normal ]; then
  ( cd "$gatesrc" && GUI_GATE_BIN="$res/gui-gate" GUI_GATE_NO_UPDATE_CHECK=1 python3 -m unittest tests.test_actions_e2e ) > "$out/bundled-gate-e2e.txt" 2>&1 \
    || fails+=("bundled gate e2e failed")
  log "bundled gate e2e: $(tail -1 "$out/bundled-gate-e2e.txt")"
fi

if [ ${#fails[@]} -gt 0 ]; then printf 'FAILURES:\n'; printf '  %s\n' "${fails[@]}" | tee "$out/failures.txt"; exit 1; fi
log "smoke ok"
