# Changelog

All notable changes to Fodder Reader are documented here. The format loosely
follows [Keep a Changelog](https://keepachangelog.com/). The project is
pre-release (`0.1.0`) and developed in milestones (M1–M6).

## Unreleased

### Planned
- **M6** — Integration pass across GNOME / KDE / XFCE / Sway.

### M5 — WebKit reader, discovery, settings
- Reader toggle switches the light (Pango) renderer to a full **WebKitGTK**
  view that loads the **live article page** (HTML/CSS/images as the site serves
  it). JavaScript stays disabled; the session is ephemeral with tracking
  prevention (no persisted cookies/cache). In-view link navigation with
  back/forward buttons and a loading spinner. Toggling back (or leaving the
  article) fully destroys the WebView so its subprocesses exit.
- Discovery-driven **Add feed**: enter a website or feed URL → Fodder resolves
  it (direct feed → title-preview confirm; multiple candidates → picker;
  none → clear message) before subscribing.
- **Preferences** dialog: autostart toggle (writes/removes the daemon's
  autostart entry) and poll-interval spin (minimum 5 minutes, saved to config).

### M4 — Viewer UI
- 3-pane libadwaita layout (`OverlaySplitView` + `NavigationSplitView`):
  feeds sidebar | article list | reader.
- Feeds sidebar with per-feed unread counts (unread feeds bold) and an
  "All Articles" aggregate; error feeds flagged with a warning icon.
- Article list per feed (newest first), unread articles bold, mark-as-read
  on open (un-bolds in place and updates badges).
- Sanitized light reader: article HTML → `ammonia` → whitelist Pango markup
  (no scripts, no network). Selectable text, clickable links.
- Actions: refresh-now (IPC `RefreshNow`), mark-all-read, open-in-browser,
  add feed (raw URL for now), remove feed (with confirm + cascade delete).
- tokio↔glib async bridge: blocking SQLite on the tokio pool, results applied
  on the GTK main thread via `spawn_future_local`.
- IPC client: `ViewerHello` handshake, live reload on `FeedsChanged`,
  navigate on `OpenAt`; the daemon rejects a duplicate viewer and raises the
  existing one.
- Empty / loading / error states for the article and reader panes.

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
