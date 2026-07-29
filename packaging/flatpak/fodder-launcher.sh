#!/bin/sh
# Flatpak entry point: start the daemon (tray + polling) and open the viewer,
# mirroring the desktop launch on a native install.
exec fodderd --open-viewer "$@"
