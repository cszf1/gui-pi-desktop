#!/usr/bin/env bash
# Manual install / update of the AppImage, keeping the previous one for rollback. The app never updates itself;
# it only shows an "update available" notice.
#   appimage-update.sh install NEW.AppImage SHA256SUMS   verify NEW against the release's SHA256SUMS, then make it
#                                                        current; the current one becomes previous
#   appimage-update.sh rollback                          swap current and previous
#   appimage-update.sh status
# Files: ${GUI_PI_APP_DIR:-~/Applications/gui-pi-desktop}/{gui-pi-desktop.AppImage, gui-pi-desktop.previous.AppImage}
# Settings and data are not touched (both versions read the same ~/.config and ~/.local/share dirs).
set -euo pipefail
NAME=gui-pi-desktop
DIR=${GUI_PI_APP_DIR:-$HOME/Applications/$NAME}
CUR=$DIR/$NAME.AppImage
PREV=$DIR/$NAME.previous.AppImage

case "${1:-}" in
  install)
    new=${2:?usage: $0 install NEW.AppImage SHA256SUMS}
    sums=${3:?the release SHA256SUMS is required (no unverified installs)}
    want=$(awk -v f="$(basename "$new")" '{p=$2; sub(/^[*]/,"",p); sub(/^\.\//,"",p); n=split(p,a,"/"); if (a[n]==f) print $1}' "$sums" | head -n1)
    [ -n "$want" ] || { echo "$(basename "$new") is not listed in $sums" >&2; exit 4; }
    have=$(sha256sum "$new" | cut -d' ' -f1)
    [ "$have" = "$want" ] || { echo "SHA256 mismatch for $new: refusing to install" >&2; exit 5; }
    mkdir -p "$DIR"
    cp -- "$new" "$DIR/.$NAME.new"
    chmod 0755 "$DIR/.$NAME.new"
    kept=""
    if [ -e "$CUR" ]; then mv -f -- "$CUR" "$PREV"; kept=" (previous kept: $PREV)"; fi
    mv -f -- "$DIR/.$NAME.new" "$CUR"
    echo "installed $CUR$kept"
    ;;
  rollback)
    [ -e "$PREV" ] || { echo "no previous version to roll back to" >&2; exit 6; }
    if [ -e "$CUR" ]; then mv -f -- "$CUR" "$DIR/.$NAME.swap"; fi
    mv -f -- "$PREV" "$CUR"
    if [ -e "$DIR/.$NAME.swap" ]; then mv -f -- "$DIR/.$NAME.swap" "$PREV"; fi
    echo "rolled back: $CUR is the previous version again"
    ;;
  status)
    for f in "$CUR" "$PREV"; do [ -e "$f" ] && echo "$f $(sha256sum "$f" | cut -d' ' -f1)"; done; true
    ;;
  *) echo "usage: $0 install NEW.AppImage SHA256SUMS | rollback | status" >&2; exit 2 ;;
esac
