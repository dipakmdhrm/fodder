#!/usr/bin/env bash
# Remove a per-user Fodder Reader install.
#
#   ./uninstall.sh           remove app files; keep feeds, database, and config
#   ./uninstall.sh --purge   also delete feeds, database, and config
set -euo pipefail

APP_ID="io.github.dipakmdhrm.Fodder"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}"

purge=false
case "${1:-}" in
  --purge)   purge=true ;;
  "")        ;;
  -h|--help) echo "usage: $0 [--purge]   (--purge also deletes feeds/database/config)"; exit 0 ;;
  *)         echo "unknown option: $1"; echo "usage: $0 [--purge]"; exit 2 ;;
esac

rm -fv "$BIN_DIR/fodderd" "$BIN_DIR/fodder"
rm -fv "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg"
for s in 16 24 32 48 64 128 256 512; do
  rm -fv "$DATA_DIR/icons/hicolor/${s}x${s}/apps/$APP_ID.png"
done
rm -fv "$DATA_DIR/applications/$APP_ID.desktop"
# The autostart entry, if the user enabled it.
rm -fv "$CONFIG_DIR/autostart/fodder.desktop"

command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$DATA_DIR/icons/hicolor" 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA_DIR/applications" 2>/dev/null || true

if $purge; then
  echo "==> Purging user data"
  rm -rfv "$DATA_DIR/fodder" "$CONFIG_DIR/fodder"
  echo "Uninstalled, and purged all feeds, database, and config."
else
  echo "Uninstalled. Feeds/database/config under $DATA_DIR/fodder and $CONFIG_DIR/fodder were left intact."
  echo "(Re-run with --purge to delete those too.)"
fi
