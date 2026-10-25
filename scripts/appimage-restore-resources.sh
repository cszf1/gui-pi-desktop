#!/usr/bin/env bash
# B3: the AppImage bundler (linuxdeploy) rewrites every ELF it finds in the AppDir, including the bundled resources
# (node, gui-gate, cua-driver, native .node modules get RUNPATH $ORIGIN and are re-linked). The app checks those
# resources against resources/SHA256SUMS at every start, so a patched AppImage refused to start any task.
#
# This puts the staged resources back byte for byte (the same files the .deb ships), checks them against their
# SHA256SUMS, and rebuilds the AppImage with its own runtime (runtime ELF + a new squashfs, same compression).
# Nothing else in the image changes (the app binary and its bundled GTK/WebKit libraries stay as linuxdeploy made them).
#
# usage: scripts/appimage-restore-resources.sh <file.AppImage> <staged resources dir (dist/resources)>
# needs: python3, squashfs-tools (unsquashfs, mksquashfs), sha256sum
set -euo pipefail
img=$(readlink -f "$1"); res=$(readlink -f "$2")
[ -f "$img" ] && [ -f "$res/SHA256SUMS" ] || { echo "usage: $0 <AppImage> <resources dir with SHA256SUMS>" >&2; exit 2; }
work=$(mktemp -d); trap 'rm -rf "$work"' EXIT

# the runtime is a plain ELF; the squashfs starts right after its section header table
off=$(python3 - "$img" <<'PY'
import struct, sys
b = open(sys.argv[1], "rb").read(64)
assert b[:4] == b"\x7fELF", "not an ELF AppImage"
if b[4] == 2:   # 64-bit
    shoff, = struct.unpack_from("<Q", b, 0x28); shentsize, shnum = struct.unpack_from("<HH", b, 0x3A)
else:
    shoff, = struct.unpack_from("<I", b, 0x20); shentsize, shnum = struct.unpack_from("<HH", b, 0x2E)
print(shoff + shentsize * shnum)
PY
)
comp=$(unsquashfs -s -o "$off" "$img" | sed -n 's/^Compression //p' | head -1)
[ -n "$comp" ] || { echo "no squashfs at offset $off" >&2; exit 1; }
unsquashfs -q -n -o "$off" -d "$work/root" "$img"
dst=$(find "$work/root/usr/lib" -maxdepth 2 -type d -name resources -path '*gui-pi-desktop*' | head -1)
[ -n "$dst" ] || { echo "no usr/lib/gui-pi-desktop/resources in the AppImage" >&2; exit 1; }
before=$( (cd "$dst" && sha256sum -c --quiet SHA256SUMS 2>/dev/null | wc -l) || true)
echo "resources changed by the bundler: $before file(s)"
rm -rf "$dst"
cp -a "$res" "$dst"
(cd "$dst" && sha256sum -c --quiet SHA256SUMS)
head -c "$off" "$img" > "$work/runtime"
mksquashfs "$work/root" "$work/fs" -root-owned -noappend -comp "$comp" -no-progress -quiet
cat "$work/runtime" "$work/fs" > "$work/out.AppImage"
chmod 0755 "$work/out.AppImage"
# verify the result the way the app will see it
unsquashfs -q -n -o "$off" -d "$work/check" "$work/out.AppImage"
chk=$(find "$work/check/usr/lib" -maxdepth 2 -type d -name resources -path '*gui-pi-desktop*' | head -1)
(cd "$chk" && sha256sum -c --quiet SHA256SUMS)
if command -v readelf >/dev/null && [ -f "$chk/gui-gate" ] && readelf -d "$chk/gui-gate" | grep -q 'RUNPATH\|RPATH'; then
  echo "gui-gate still has an RPATH/RUNPATH" >&2; exit 1
fi
mv "$work/out.AppImage" "$img"
echo "ok: $(basename "$img"): resources restored ($(wc -l < "$res/SHA256SUMS") files match SHA256SUMS), compression $comp"
