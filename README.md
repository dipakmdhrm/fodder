# Fodder Reader

A lightweight RSS / Atom / JSON-Feed reader for Linux desktops (GNOME, KDE,
XFCE/Sway), written in Rust.

Fodder is built around a small, always-resident **daemon** that does the
polling and notifying in the background, and a **viewer** that is spawned on
demand and — by default — kept resident between opens, so reopening the window
is instant. Want a leaner footprint? A **"Low memory mode"** preference frees
the viewer when its window is closed, dropping idle memory to ~20–40 MB — versus
a couple hundred MB with the viewer resident, or around ½ GB when the web view is
open — in exchange for a slightly slower reopen.

## Architecture

A Cargo workspace with three crates:

| Crate | Kind | Responsibility |
|-------|------|----------------|
| `core` (`fodder-core`) | library | Models, SQLite store + migrations, HTTP poller (conditional GET), feed discovery, config, IPC protocol. |
| `fodderd` | binary | Headless daemon: tokio poll loop, shared SQLite (WAL), desktop notifications, single-instance IPC socket, system-tray icon, and on-demand viewer spawning. |
| `fodder` | binary | GTK4 + libadwaita viewer: 3-pane UI (feeds / articles / reader) with a sanitized light reader, an optional locked-down WebKit view, discovery-driven subscribe, and a header-bar menu (Preferences / About). |

**Process model.** `fodderd` is the primary process and stays resident. The
`fodder` viewer is launched on demand and, by default, hidden and kept resident
on close for an instant reopen (or, with **Low memory mode** enabled, terminated
on close to free its memory). Exactly one daemon and one viewer are enforced via
a Unix socket in `$XDG_RUNTIME_DIR`, which also carries daemon↔viewer IPC.

**Storage.**
- Config: `~/.config/fodder/config.toml`
- Database: `~/.local/share/fodder/db.sqlite` (SQLite, WAL mode)
- Runtime socket: `$XDG_RUNTIME_DIR/fodder/daemon.sock`

App ID: `io.github.dipakmdhrm.Fodder`.

## Requirements

- Rust (stable, 2021 edition) + Cargo
- System libraries: `libgtk-4-dev`, `libadwaita-1-dev`, `libwebkitgtk-6.0-dev`
  (GTK 4.14+, libadwaita 1.5+, WebKitGTK 6.0)

## Build & test

```bash
cargo build --workspace     # build all three crates
cargo test --workspace      # run the test suite
```

## Install

Fodder needs a recent distro (GTK 4.14+, libadwaita 1.5+, WebKitGTK 6.0):
Ubuntu 24.04+, Fedora 40+, Debian 13, Arch, or any distro via Flatpak.

### Debian / Ubuntu (`.deb`, with automatic updates)

Add the signed apt repository once, then install — `apt upgrade` tracks
future releases:

```bash
curl -fsSL https://dipakmdhrm.github.io/fodder/key.gpg | sudo gpg --dearmor -o /etc/apt/keyrings/fodder.gpg
echo "deb [signed-by=/etc/apt/keyrings/fodder.gpg] https://dipakmdhrm.github.io/fodder stable main" | sudo tee /etc/apt/sources.list.d/fodder.list
sudo apt update && sudo apt install fodder
```
(Or download the `.deb` from a [release](https://github.com/dipakmdhrm/fodder/releases)
and `sudo apt install ./fodder_*.deb` — the postinstall adds the repo for you.)

### Fedora (`.rpm`) / Arch (`.pkg.tar.zst`)

Download the matching package from a
[release](https://github.com/dipakmdhrm/fodder/releases):

```bash
sudo dnf install ./fodder-*.rpm            # Fedora
sudo pacman -U ./fodder-*.pkg.tar.zst      # Arch
```

### Flatpak (any distro, with automatic updates)

```bash
flatpak remote-add --if-not-exists fodder https://dipakmdhrm.github.io/fodder/fodder.flatpakrepo
flatpak install fodder io.github.dipakmdhrm.Fodder
```
`flatpak update` then keeps it current.

> **Flathub** (pending review): once accepted, `flatpak install flathub
> io.github.dipakmdhrm.Fodder` will install from Flathub instead of this repo.
> The Flathub build manifest and its vendored crate sources live in
> `packaging/flatpak/flathub/`.

### From source (per-user, no packaging)

```bash
./install.sh                # release build → ~/.local/bin, icon + .desktop → ~/.local/share
./uninstall.sh              # remove app files; keep your feeds/config
./uninstall.sh --purge      # also delete feeds, database, and config
```
Ensure `~/.local/bin` is on your `PATH`.

Release/packaging details live in [docs/RELEASING.md](docs/RELEASING.md); for
cross-desktop verification see [docs/cross-desktop-testing.md](docs/cross-desktop-testing.md).

## Running (current state)

The daemon runs, polls feeds, shows a tray icon, and spawns the viewer on
demand. The viewer is a working 3-pane reader (add/rename/remove feeds, read
articles, mark read, refresh). Right-click a feed in the sidebar to rename or
delete it. The example CLI tools remain handy for scripting.

```bash
# Run the daemon (foreground, with logs). Shows a tray icon where supported;
# left-click or the tray's "Open Fodder" spawns the viewer.
RUST_LOG=info cargo run -p fodderd

# Or launch the viewer directly (it connects back to the daemon)
cargo run -p fodder

# Print version info and exit (either binary; also accepts -V)
fodderd --version
fodder --version

# Drive the daemon over IPC (in another terminal)
cargo run -p fodderd --example ctl -- ping
cargo run -p fodderd --example ctl -- subscribe <feed-url> "<title>"
cargo run -p fodderd --example ctl -- refresh
cargo run -p fodderd --example ctl -- open              # spawn/raise the viewer
cargo run -p fodderd --example ctl -- list              # show subscribed feeds
cargo run -p fodderd --example ctl -- rm <id>           # unsubscribe (cascade)
cargo run -p fodderd --example ctl -- autostart on|off|status

# Exercise core feed discovery + polling directly against a URL
cargo run -p fodder-core --example poll -- https://blog.rust-lang.org/
```

> Note: `ctl subscribe` inserts a raw URL and bypasses feed discovery/validation
> (that's the job of the viewer's add-feed flow in M5). Use it with resolved
> feed URLs.

## Project history & roadmap

See [CHANGELOG.md](CHANGELOG.md) for what has shipped so far and the planned
milestones.

## License

MIT
