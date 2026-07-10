# Fodder

A resource-frugal RSS/Atom feed reader for Linux, built with GTK4 + libadwaita.

## Design

Fodder is two processes sharing one SQLite database:

- **`fodder`** — the GTK4/libadwaita viewer. A three-pane layout (feeds → items → article) that adapts to narrow windows by collapsing into stacked navigation.
- **`fodder-daemon`** — a headless poller (~a few MB of RAM, no GTK linked). It fetches feeds on the configured interval, stores new items, sends desktop notifications, and owns the tray icon.

When "Run in background" is enabled, closing the window exits the viewer process entirely — only the daemon stays resident, so the UI consumes literally zero RAM/CPU while in the background.

Articles render natively (sanitized HTML → GTK text view) by default. A per-article toggle switches to a WebKitGTK view for full fidelity; toggling back terminates the WebKit processes and reclaims their memory.

### Verifying WebKit teardown

After switching an article from web view back to native view, confirm no WebKit
helper processes remain:

```sh
sleep 2 && pgrep -a -f 'WebKit(Web|Network)Process' || echo "clean"
```

## Features

- Subscribe to multiple RSS/Atom feeds
- Update interval: 15 min / 30 min / 1 h / 4 h / 12 h / daily
- Run in background, autostart on login, desktop notifications — all toggleable in Preferences
- Tray icon (StatusNotifierItem; on vanilla GNOME Shell this requires the
  [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/))

## Building

Requires Rust (stable) and the GTK dev packages:

```sh
# Debian/Ubuntu/Mint
sudo apt install build-essential pkg-config libgtk-4-dev libadwaita-1-dev libwebkitgtk-6.0-dev
cargo build --release
```

Run tests with `cargo test --workspace`.

## Runtime requirements

GTK 4.14+, libadwaita 1.5+, webkitgtk-6.0 (Ubuntu 24.04+, Debian 12+, Fedora 40+, Arch).
A D-Bus session bus is expected (notifications, tray, viewer↔daemon coordination);
without one, polling still works and the viewer refreshes from the database directly.

## Packaging

CI builds and test-installs `.deb` (Ubuntu), `.rpm` (Fedora), and pacman
(Arch) packages on every PR; releases attach all three.
