# Changelog

All notable changes to Fodder Reader are documented here. The format loosely
follows [Keep a Changelog](https://keepachangelog.com/). The project is
pre-release (`0.1.0`) and developed in milestones (M1–M6).

## Unreleased

### Planned
- **M4** — Viewer: 3-pane libadwaita UI, feed/article lists, sanitized light
  reader, mark-read / mark-all-read / refresh / open-in-browser.
- **M5** — Viewer: WebKit reader toggle, discovery-driven subscribe/remove,
  settings (autostart, poll interval).
- **M6** — Integration pass across GNOME / KDE / XFCE / Sway.

### M3 — Tray & viewer process management
- StatusNotifierItem system tray via `ksni` (Open / Refresh all / Quit;
  left-click opens the viewer). Registration is best-effort: on hosts without
  an SNI tray it degrades gracefully and keeps polling/notifying.
- On-demand viewer process management: spawn the single `fodder` child on an
  open request (navigation target passed as CLI args), enforce one viewer, reap
  it on exit, and terminate it on daemon shutdown.
- Shared autostart writer (`core::autostart`): install/remove
  `~/.config/autostart/fodder.desktop` (launches the daemon). Exposed via
  `ctl autostart on|off|status`.
- Unified shutdown (SIGINT/SIGTERM or the tray's Quit) that reaps the viewer,
  tears down the tray, and removes the socket.

### M2 — Daemon
- Single-instance guard via the runtime socket with stale-socket probe.
- IPC server + dispatch (ping, subscribe, refresh, viewer routing).
- Poll scheduler honoring per-feed `next_poll_at`; `refresh` force-polls all
  feeds regardless of schedule.
- Batched, actionable desktop notifications (notify-rust); the click action
  routes to the newest article.
- `ctl` example tool (`ping` / `subscribe` / `refresh` / `open` / `list` /
  `rm`) for manual testing.

### M1 — Core foundations
- Cargo workspace with three crates: `core`, `fodderd`, `fodder`.
- SQLite store with WAL + `user_version` migrations; feed/article CRUD; GUID
  dedupe via `INSERT OR IGNORE` on `UNIQUE(feed_id, guid)`.
- HTTP poller: conditional GET (ETag / Last-Modified / 304), 429 + Retry-After,
  per-feed exponential backoff, bounded-concurrency fetch, `feed-rs` parsing.
- Feed discovery (direct feed + `<link rel=alternate>` extraction).
- TOML config (poll interval validated to ≥ 5 minutes).
- IPC protocol: length-prefixed JSON framing over a Unix socket.
- `poll` example tool for exercising discovery + polling against a URL.
- 36 tests (31 unit + 5 wiremock integration), zero warnings.
