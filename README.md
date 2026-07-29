# Fodder Reader

A lightweight RSS / Atom / JSON-Feed reader for Linux desktops (GNOME, KDE,
XFCE/Sway), written in Rust.

Fodder is built around a small, always-resident **daemon** that does the
polling and notifying, and a **viewer** that is spawned only on demand and
freed when closed — so idle memory stays low while you still get live updates.

## Architecture

A Cargo workspace with three crates:

| Crate | Kind | Responsibility |
|-------|------|----------------|
| `core` (`fodder-core`) | library | Models, SQLite store + migrations, HTTP poller (conditional GET), feed discovery, config, IPC protocol. |
| `fodderd` | binary | Headless daemon: tokio poll loop, shared SQLite (WAL), desktop notifications, single-instance IPC socket. Owns the tray and spawns the viewer *(tray/spawn land in M3)*. |
| `fodder` | binary | GTK4 + libadwaita viewer *(the 3-pane UI lands in M4)*. |

**Process model.** `fodderd` is the primary process and stays resident. The
`fodder` viewer is launched on demand and terminated on close. Exactly one
daemon and one viewer are enforced via a Unix socket in `$XDG_RUNTIME_DIR`,
which also carries daemon↔viewer IPC.

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

## Running (current state)

The daemon runs and polls feeds today; the viewer is still a placeholder
window. Until the GTK UI lands, a couple of example tools let you exercise the
working parts.

```bash
# Run the daemon (foreground, with logs)
RUST_LOG=info cargo run -p fodderd

# Drive the daemon over IPC (in another terminal)
cargo run -p fodderd --example ctl -- ping
cargo run -p fodderd --example ctl -- subscribe <feed-url> "<title>"
cargo run -p fodderd --example ctl -- refresh
cargo run -p fodderd --example ctl -- list        # show subscribed feeds
cargo run -p fodderd --example ctl -- rm <id>     # unsubscribe (cascade)

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
