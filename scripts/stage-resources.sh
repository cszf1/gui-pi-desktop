#!/usr/bin/env bash
# Lays out dist/resources/ for the Tauri bundle: node, Pi, the gui-pi extension, prompts, skills, gui-gate and the
# pinned Cua Driver. Every download is checked against a pinned SHA256; every staged file goes into SHA256SUMS, which
# the app checks at every start.
#   usage: scripts/stage-resources.sh <gui-gate checkout> [PLATFORM] [gui-gate binary]
#   PLATFORM: linux-x86_64 (default) | windows-x86_64 | macos-aarch64 | macos-x86_64
#   gui-gate binary: default <checkout>/target/release/gui-gate[.exe]
# Needs bash, curl, tar, npm and python3 (PYTHON=python on Windows runners).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
GATE=${1:?path to the gui-gate checkout (built with cargo build --release)}
PLAT=${2:-linux-x86_64}
PY=${PYTHON:-python3}
OUT=$ROOT/dist/resources
NODE_VER=v22.23.4
# official SHASUMS256.txt of https://nodejs.org/dist/v22.23.4/ (latest v22 "Jod" LTS on 2026-10-26)
case "$PLAT" in
  linux-x86_64)   NODE_ASSET=node-$NODE_VER-linux-x64.tar.xz;   NODE_SHA=1805bcd35e6815559bd2eb97db438d3fdb0eb3a5e6a977b7c1badca76337e9ce; DRV=linux-x86_64; EXE="";;
  windows-x86_64) NODE_ASSET=node-$NODE_VER-win-x64.zip;        NODE_SHA=cbc74b5e0982c9dbb41525fd8fb5dcd32fbee237863f7829072f4639a1ce5f20; DRV=windows-x86_64; EXE=".exe";;
  macos-aarch64)  NODE_ASSET=node-$NODE_VER-darwin-arm64.tar.xz; NODE_SHA=037650d0839f457747396f052073fb48333d31bf5abd3c98b74da20d89ebb1ad; DRV=macos-universal; EXE="";;
  macos-x86_64)   NODE_ASSET=node-$NODE_VER-darwin-x64.tar.xz;   NODE_SHA=f42abca7aaac61efc57d49fb42ea84fc53bbeba9a749cb03a291ce6e4e7a69c1; DRV=macos-universal; EXE="";;
  *) echo "unknown platform $PLAT" >&2; exit 2;;
esac
GATE_BIN=${3:-$GATE/target/release/gui-gate$EXE}

rm -rf "$OUT" && mkdir -p "$OUT"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# node: only the runtime (node[.exe] + LICENSE) at node/bin/node[.exe] on every platform
curl -fsSL --retry 3 -o "$tmp/$NODE_ASSET" "https://nodejs.org/dist/$NODE_VER/$NODE_ASSET"
"$PY" - "$tmp/$NODE_ASSET" "$NODE_SHA" <<'EOF'
import hashlib, sys
h = hashlib.sha256(open(sys.argv[1], "rb").read()).hexdigest()
if h != sys.argv[2]:
    raise SystemExit(f"{sys.argv[1]}: sha256 {h} != pinned {sys.argv[2]}")
print(f"{sys.argv[1]}: sha256 OK")
EOF
mkdir -p "$OUT/node/bin" "$tmp/node"
if [ "$PLAT" = windows-x86_64 ]; then
  "$PY" -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$tmp/$NODE_ASSET" "$tmp/node"
else
  tar -xJf "$tmp/$NODE_ASSET" -C "$tmp/node"
fi
nd=$(ls -d "$tmp"/node/node-*)
if [ "$PLAT" = windows-x86_64 ]; then cp "$nd/node.exe" "$OUT/node/bin/node.exe"; else cp -p "$nd/bin/node" "$OUT/node/bin/node"; fi
cp "$nd/LICENSE" "$OUT/node/LICENSE"

# Pi (exact versions from the lock file)
mkdir -p "$OUT/pi" && cp "$ROOT/packaging/pi/package.json" "$ROOT/packaging/pi/package-lock.json" "$OUT/pi/"
(cd "$OUT/pi" && npm ci --omit=dev --ignore-scripts --no-audit --no-fund)
test -f "$OUT/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"

# extension, prompt, skills
mkdir -p "$OUT/gui-pi" && cp -r "$ROOT/packages/gui-pi/index.ts" "$ROOT/packages/gui-pi/src" "$ROOT/packages/gui-pi/package.json" "$OUT/gui-pi/"
cp -r "$ROOT/prompts" "$ROOT/skills" "$OUT/"

# gate + pinned driver (fetch_driver.py checks scripts/cua-driver.lock hashes, the release SHA256SUMS and, with
# REQUIRE_SIG, the Sigstore bundle); macOS: Cua's signed CuaDriver.app, byte for byte (ditto)
cp "$GATE_BIN" "$OUT/gui-gate$EXE" && chmod 0755 "$OUT/gui-gate$EXE"
"$PY" "$GATE/scripts/fetch_driver.py" "$DRV" "$OUT" ${REQUIRE_SIG:+--require-signature}
case "$PLAT" in
  macos-*) test -x "$OUT/CuaDriver.app/Contents/MacOS/cua-driver";;
  *) test -f "$OUT/cua-driver/cua-driver$EXE";;
esac

# every file, LF line ends, forward slashes, sorted: the app checks them all at start (integrity.rs)
"$PY" - "$OUT" "$PLAT" <<'EOF'
import hashlib, os, sys
out, plat = sys.argv[1], sys.argv[2]
rows, longest = [], ""
for r, ds, fs in os.walk(out):
    ds.sort()
    for f in sorted(fs):
        p = os.path.join(r, f)
        rel = os.path.relpath(p, out).replace(os.sep, "/")
        if rel == "SHA256SUMS":
            continue
        h = hashlib.sha256()
        with open(p, "rb") as fh:
            for b in iter(lambda: fh.read(1 << 20), b""):
                h.update(b)
        rows.append(f"{h.hexdigest()}  ./{rel}\n")
        longest = max(longest, rel, key=len)
rows.sort(key=lambda l: l[66:])
with open(os.path.join(out, "SHA256SUMS"), "w", newline="\n") as f:
    f.writelines(rows)
print(f"{len(rows)} files in SHA256SUMS; longest relative path {len(longest)}: {longest}")
# Windows: the per-user install dir (%LOCALAPPDATA%\gui-pi-desktop\resources\) plus this must stay under MAX_PATH (260)
if plat.startswith("windows") and len(longest) > 180:
    raise SystemExit(f"a resource path is {len(longest)} characters; with the install dir it would pass Windows' 260-character limit")
EOF
echo "staged $(du -sh "$OUT" | cut -f1) in $OUT for $PLAT"
