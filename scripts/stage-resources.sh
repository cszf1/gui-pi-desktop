#!/usr/bin/env bash
# Lays out dist/resources/ for the Tauri bundle: node, Pi, the gui-pi extension, prompts, skills, gui-gate and the
# pinned Cua Driver. Every download is checked against a pinned SHA256. Linux x86_64 for now.
#   usage: scripts/stage-resources.sh <path to a built gui-gate checkout>
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
GATE=${1:?path to the gui-gate checkout (built with cargo build --release)}
OUT=$ROOT/dist/resources
NODE_VER=v22.23.4
NODE_SHA=1805bcd35e6815559bd2eb97db438d3fdb0eb3a5e6a977b7c1badca76337e9ce  # node-v22.23.4-linux-x64.tar.xz (SHASUMS256)

rm -rf "$OUT" && mkdir -p "$OUT"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# node
curl -fsSL -o "$tmp/node.tar.xz" "https://nodejs.org/dist/$NODE_VER/node-$NODE_VER-linux-x64.tar.xz"
echo "$NODE_SHA  $tmp/node.tar.xz" | sha256sum -c -
mkdir -p "$OUT/node" && tar -xJf "$tmp/node.tar.xz" -C "$OUT/node" --strip-components=1
rm -rf "$OUT/node/include" "$OUT/node/share" "$OUT/node/lib/node_modules/npm" "$OUT/node/lib/node_modules/corepack" \
       "$OUT/node/bin/npm" "$OUT/node/bin/npx" "$OUT/node/bin/corepack"

# Pi (exact versions from the lock file)
mkdir -p "$OUT/pi" && cp "$ROOT/packaging/pi/package.json" "$ROOT/packaging/pi/package-lock.json" "$OUT/pi/"
(cd "$OUT/pi" && npm ci --omit=dev --ignore-scripts --no-audit --no-fund)
test -f "$OUT/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js"

# extension, prompt, skills
mkdir -p "$OUT/gui-pi" && cp -r "$ROOT/packages/gui-pi/index.ts" "$ROOT/packages/gui-pi/src" "$ROOT/packages/gui-pi/package.json" "$OUT/gui-pi/"
cp -r "$ROOT/prompts" "$ROOT/skills" "$OUT/"

# gate + pinned driver (fetch_driver.py checks scripts/cua-driver.lock hashes and the release SHA256SUMS)
install -m 0755 "$GATE/target/release/gui-gate" "$OUT/gui-gate"
python3 "$GATE/scripts/fetch_driver.py" linux-x86_64 "$OUT" ${REQUIRE_SIG:+--require-signature}
test -x "$OUT/cua-driver/cua-driver"

(cd "$OUT" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
echo "staged $(du -sh "$OUT" | cut -f1) in $OUT"
