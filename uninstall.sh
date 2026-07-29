#!/usr/bin/env bash
# Remove a per-user Fodder Reader install.
set -euo pipefail

APP_ID="io.github.dipakmdhrm.Fodder"
BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"

rm -fv "$BIN_DIR/fodderd" "$BIN_DIR/fodder"
rm -fv "$DATA_DIR/icons/hicolor/scalable/apps/$APP_ID.svg"
rm -fv "$DATA_DIR/applications/$APP_ID.desktop"
# The autostart entry, if the user enabled it.
rm -fv "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/fodder.desktop"

command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$DATA_DIR/icons/hicolor" 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$DATA_DIR/applications" 2>/dev/null || true

echo "Uninstalled. Your feeds/config under ~/.local/share/fodder and ~/.config/fodder were left intact."
