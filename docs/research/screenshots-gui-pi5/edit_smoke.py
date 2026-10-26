def sub(f, old, new, cnt=1):
    s = open(f).read(); n = s.count(old); assert n == cnt, (f, old[:60], n); open(f, 'w').write(s.replace(old, new))

f = 'scripts/ci/mac-smoke.sh'
sub(f, '''# usage: scripts/ci/mac-smoke.sh <dmg> <out dir> normal|testhooks <gui-gate checkout>
set -uo pipefail
dmg=$1; out=$2; variant=$3; gatesrc=$4
mkdir -p "$out"
fails=()
log() { echo "$*" | tee -a "$out/summary.txt"; }
''', '''# usage: scripts/ci/mac-smoke.sh <dmg> <out dir> normal|testhooks <gui-gate checkout> [SHA256SUMS of the packages]
set -uo pipefail
dmg=$1; out=$2; variant=$3; gatesrc=$4; sums=${5:-}
mkdir -p "$out"
fails=()
log() { echo "$*" | tee -a "$out/summary.txt"; }

# the .dmg is the one the release's SHA256SUMS names (the Intel job runs the artifact built on arm64)
if [ -n "$sums" ]; then
  want=$(awk -v n="$(basename "$dmg")" '$2 == n || $2 == "*"n {print $1}' "$sums")
  got=$(shasum -a 256 "$dmg" | cut -d' ' -f1)
  if [ -n "$want" ] && [ "$want" = "$got" ]; then log "SHA256SUMS: $(basename "$dmg") ok ($got)"; else fails+=("SHA256SUMS: $(basename "$dmg") is $got, the list says '${want:-nothing}'"); fi
fi
''')
sub(f, '''codesign -dv "$app" > "$out/codesign-app.txt" 2>&1; log "gui-pi-desktop.app signature: $(grep -E 'Signature|flags' "$out/codesign-app.txt" | tr '\\n' ' ')"
if codesign --verify --deep --strict --verbose=2 "$res/CuaDriver.app" > "$out/codesign-cuadriver.txt" 2>&1; then
  log "CuaDriver.app (nested): signature valid ($(codesign -dv "$res/CuaDriver.app" 2>&1 | grep -E '^Authority=' | head -1))"
else
  fails+=("the nested CuaDriver.app signature is no longer valid"); cat "$out/codesign-cuadriver.txt"
fi''', '''codesign -dvv "$app" > "$out/codesign-app.txt" 2>&1; log "gui-pi-desktop.app signature (unsigned by design; at most the linker's ad-hoc one): $(grep -E 'Signature|flags|not signed' "$out/codesign-app.txt" | tr '\\n' ' ')"
# Cua's own Developer ID signature on the nested CuaDriver.app must survive the bundling byte for byte (symlinks in
# its bundle included): a broken or re-signed CuaDriver loses its macOS permission identity (com.trycua.driver)
if codesign --verify --deep --strict --verbose=2 "$res/CuaDriver.app" > "$out/codesign-cuadriver.txt" 2>&1; then
  codesign -dvv "$res/CuaDriver.app" > "$out/codesign-cuadriver-id.txt" 2>&1
  auth=$(grep -m1 '^Authority=' "$out/codesign-cuadriver-id.txt"); team=$(grep -m1 '^TeamIdentifier=' "$out/codesign-cuadriver-id.txt"); ident=$(grep -m1 '^Identifier=' "$out/codesign-cuadriver-id.txt")
  log "CuaDriver.app (nested): signature valid (--deep --strict); $ident; $auth; $team"
  case "$auth" in "Authority=Developer ID Application"*) ;; *) fails+=("the nested CuaDriver.app is not signed with a Developer ID any more ($auth)");; esac
  grep -q 'Signature=adhoc' "$out/codesign-cuadriver-id.txt" && fails+=("the nested CuaDriver.app was re-signed ad hoc")
else
  fails+=("the nested CuaDriver.app signature is no longer valid"); cat "$out/codesign-cuadriver.txt"
fi
log "CuaDriver.app symlinks in the installed app: $(find "$res/CuaDriver.app" -type l | wc -l | tr -d ' ')"''')
sub(f, '''sys.exit(1 if bad else 0)
EOF
else''', '''sys.exit(1 if bad else 0)
EOF
  # each popup's page ended up at least as tall as its content (macOS once came out a title bar short)
  python3 - "$se" >> "$out/summary.txt" <<'EOF' || fails+=("a popup is shorter than its content (see summary.txt)")
import re, sys
last = {}
for line in open(sys.argv[1]):
    m = re.search(r"\\[popup\\] (\\S+) fit to 480x(\\d+) \\(content (\\d+), ok (\\w+), seen (-?\\d+)\\)", line)
    if m:
        last[m.group(1)] = (int(m.group(3)), int(m.group(5)))
bad = 0
for k, (need, seen) in sorted(last.items()):
    ok = seen >= need - 1
    print(f"popup height {k}: content {need}, page {seen}: {'ok' if ok else 'SHORT'}")
    bad += not ok
sys.exit(1 if bad or len(last) < 4 else 0)
EOF
else''')

f = '.github/workflows/macos.yml'
sub(f, '''      - name: checksums (resources.SHA256SUMS = what the app checks at start; SHA256SUMS = the packages)''', '''      - name: the nested CuaDriver.app is byte for byte the staged one (symlinks kept, nothing re-signed)
        run: python3 scripts/ci/tree-same.py dist/resources/CuaDriver.app desktop/target/universal-apple-darwin/release/bundle/macos/gui-pi-desktop.app/Contents/Resources/resources/CuaDriver.app
      - name: checksums (resources.SHA256SUMS = what the app checks at start; SHA256SUMS = the packages)''')
sub(f, '''"${{ matrix.variant }}" "$GITHUB_WORKSPACE/gate"''', '''"${{ matrix.variant }}" "$GITHUB_WORKSPACE/gate" "$GITHUB_WORKSPACE/SHA256SUMS"''')
sub(f, '''pkg/*.dmg 2>/dev/null | head -1)" "$GITHUB_WORKSPACE/smoke" normal "$GITHUB_WORKSPACE/gate"''', '''pkg/*.dmg 2>/dev/null | head -1)" "$GITHUB_WORKSPACE/smoke" normal "$GITHUB_WORKSPACE/gate" "$GITHUB_WORKSPACE/pkg/SHA256SUMS"''')
