#!/usr/bin/env bash
# Removes gui-pi-desktop's OWN files for the current user, and nothing else.
#   uninstall.sh [--print-paths] [--dry-run] [--keys]
#     --print-paths  list what would be removed (one path per line) and exit
#     --dry-run      say what would be removed, remove nothing
#     --keys         also delete the app's keychain entries (service "gui-pi-desktop", via secret-tool)
# Scope (the guard below refuses anything else):
#   ~/.config/io.github.cszf1.gui-pi-desktop       settings (no keys)
#   ~/.local/share/io.github.cszf1.gui-pi-desktop  app data: Pi agent dir, sessions, screenshots, logs
#   ~/.cache/io.github.cszf1.gui-pi-desktop        caches
#   $XDG_RUNTIME_DIR/gui-pi-desktop                 sockets and task files of a running app
#   ~/Applications/gui-pi-desktop                   AppImages installed by appimage-update.sh (current + previous)
#   ~/.local/share/applications/io.github.cszf1.gui-pi-desktop.desktop
# Never touched: a Cua Driver installed before or outside this app (~/.local/bin/cua-driver, ~/.cua,
# /usr/local/bin/cua-driver, ...; the app only uses the driver inside its own bundle), the user's documents, Pi's
# own ~/.pi, anything under /usr or /opt (a .deb is removed with apt, which keeps user data).
set -euo pipefail
ID=io.github.cszf1.gui-pi-desktop
NAME=gui-pi-desktop
: "${HOME:?HOME is not set}"
CONFIG=${XDG_CONFIG_HOME:-$HOME/.config}
DATA=${XDG_DATA_HOME:-$HOME/.local/share}
CACHE=${XDG_CACHE_HOME:-$HOME/.cache}
APPS=${GUI_PI_APP_DIR:-$HOME/Applications/$NAME}

paths=("$CONFIG/$ID" "$DATA/$ID" "$CACHE/$ID" "$APPS" "$DATA/applications/$ID.desktop")
if [ -n "${XDG_RUNTIME_DIR:-}" ]; then paths+=("$XDG_RUNTIME_DIR/$NAME"); fi

# The guard: every path must be absolute, inside $HOME or $XDG_RUNTIME_DIR, end in one of the app's own names,
# so a Cua Driver install (or anything else of the user's) can never be on the list.
guard() {
  local p=$1 base
  case "$p" in /*) ;; *) echo "refusing relative path: $p" >&2; return 1 ;; esac
  case "$p" in *"/../"*|*"/./"*|*/..|*/.) echo "refusing: $p" >&2; return 1 ;; esac
  case "$p" in "$HOME"/*) ;; "${XDG_RUNTIME_DIR:-/nonexistent}"/*) ;; *) echo "refusing outside HOME: $p" >&2; return 1 ;; esac
  base=${p##*/}
  case "$base" in "$ID"|"$NAME"|"$ID.desktop") ;; *) echo "refusing foreign name: $p" >&2; return 1 ;; esac
  return 0
}

mode=remove keys=0
for a in "$@"; do
  case "$a" in
    --print-paths) mode=print ;;
    --dry-run) mode=dry ;;
    --keys) keys=1 ;;
    *) echo "usage: $0 [--print-paths] [--dry-run] [--keys]" >&2; exit 2 ;;
  esac
done

for p in "${paths[@]}"; do guard "$p" || exit 3; done
if [ "$mode" = print ]; then printf '%s\n' "${paths[@]}"; exit 0; fi
for p in "${paths[@]}"; do
  if [ -e "$p" ] || [ -L "$p" ]; then
    if [ "$mode" = dry ]; then echo "would remove $p"; else rm -rf -- "$p"; echo "removed $p"; fi
  fi
done
if [ "$keys" = 1 ]; then
  if command -v secret-tool >/dev/null 2>&1; then
    if [ "$mode" = dry ]; then echo "would delete keychain entries of service $NAME"; else secret-tool clear service "$NAME" || true; echo "keychain entries of $NAME deleted"; fi
  else
    echo "secret-tool not found: delete the \"$NAME\" entries in your keyring app yourself" >&2
  fi
fi
