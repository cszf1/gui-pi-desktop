#!/usr/bin/env bash
# Version release on a tag push (refs/tags/vX.Y.Z): every platform workflow adds its packages to ONE release.
#   normal build    -> release vX.Y.Z (a full release, the one the app's update notice reads)
#   test-hooks build -> prerelease vX.Y.Z-testhooks (scripted person answers; test machines only)
# The per-platform checksum files get the platform as a suffix (SHA256SUMS.linux, resources.SHA256SUMS.windows, ...)
# so three workflows never overwrite each other's. Whoever comes first creates the release; the others upload.
# usage: publish-version.sh PLATFORM normal|testhooks FILE...
set -euo pipefail
plat=${1:?platform}; variant=${2:?variant}; shift 2
case "${GITHUB_REF:-}" in refs/tags/v*) ;; *) echo "not a version tag push: ${GITHUB_REF:-?}" >&2; exit 1 ;; esac
ver="$GITHUB_REF_NAME"
tag="$ver"; pre=(--latest)
if [ "$variant" = testhooks ]; then tag="$ver-testhooks"; pre=(--prerelease); fi
stage=$(mktemp -d)
for f in "$@"; do
  b=$(basename "$f")
  case "$b" in SHA256SUMS|resources.SHA256SUMS) cp "$f" "$stage/$b.$plat" ;; *) cp "$f" "$stage/$b" ;; esac
done
notes="gui-pi-desktop $ver: Linux (AppImage + .deb), Windows x86_64 (NSIS + MSI, unsigned), macOS 13+ universal (.dmg, unsigned, not notarized: docs/design.md §6.8).
Bundles gui-gate ${GATE_REF::7} (v0.10.2, the last standalone gui-gate release; the gate now evolves with this app) and Cua Driver 0.38.0.
Check a download against SHA256SUMS.<platform>; the app checks its bundled files against resources.SHA256SUMS at start.
Changes: CHANGELOG.md."
[ "$variant" = testhooks ] && notes="TEST HOOKS ON: scripted person answers and --demo-popups; for test machines only, never for real use.
$notes"
for i in 1 2 3 4 5 6; do
  gh release view "$tag" >/dev/null 2>&1 && break
  gh release create "$tag" "${pre[@]}" --target "$GITHUB_SHA" --title "gui-pi-desktop $tag" --notes "$notes" && break || sleep $((i * 5))
done
gh release upload "$tag" "$stage"/* --clobber
gh release view "$tag" --json assets --jq '.assets[].name'
