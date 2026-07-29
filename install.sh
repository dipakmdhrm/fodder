#!/usr/bin/env bash
# Per-user install for Fodder Reader (no sudo).
#
#   ./install.sh          build in release and install
#   ./install.sh --debug  install an existing debug build (faster, for testing)
set -euo pipefail

APP_ID="io.github.dipakmdhrm.Fodder"
HERE="$(cd "$(dirname "$0")" && pwd)"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
ICON_DIR="$DATA_DIR/icons/hicolor/scalable/apps"
APPS_DIR="$DATA_DIR/applications"

profile="release"
target="$HERE/target/release"
if [[ "${1:-}" == "--debug" ]]; then
  profile="debug"
  target="$HERE/target/debug"
fi

if [[ "$profile" == "release" ]]; then
  echo "==> Building (release)…"
  ( cd "$HERE" && cargo build --release --workspace )
fi

echo "==> Installing binaries to $BIN_DIR"
mkdir -p "$BIN_DIR"
install -m 0755 "$target/fodderd" "$BIN_DIR/fodderd"
install -m 0755 "$target/fodder"  "$BIN_DIR/fodder"

echo "==> Installing icon to $ICON_DIR"
mkdir -p "$ICON_DIR"
install -m 0644 "$HERE/data/icons/hicolor/scalable/apps/$APP_ID.svg" "$ICON_DIR/$APP_ID.svg"

echo "==> Installing desktop entry to $APPS_DIR"
mkdir -p "$APPS_DIR"
install -m 0644 "$HERE/data/applications/$APP_ID.desktop" "$APPS_DIR/$APP_ID.desktop"

# Refresh caches (best-effort; harmless if the tools are missing).
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qtf "$DATA_DIR/icons/hicolor" 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$APPS_DIR" 2>/dev/null || true

echo
echo "Installed. Make sure $BIN_DIR is on your PATH."
echo "Launch 'Fodder Reader' from your app menu, or run: fodderd  (then click the tray icon)."
