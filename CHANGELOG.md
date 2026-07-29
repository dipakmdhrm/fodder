# Changelog

All notable changes to Fodder Reader are documented here. The format loosely
follows [Keep a Changelog](https://keepachangelog.com/). The project is
pre-release (`0.1.0`) and developed in milestones (M1–M6).

## Unreleased

### Build & release
- The Arch package now builds again: it links the **system** SQLite (its
  PKGBUILD drops rusqlite's `bundled` feature) instead of the vendored copy,
  whose static symbols were dropped by makepkg's hardening link flags.
- Every package (`.deb`, `.rpm`, Arch, Flatpak) is now built on each pull
  request via a shared reusable workflow (`build-packages.yml`) that the
  Release pipeline also calls, so a release tag repeats an already-green build.

## 0.1.1

### Build & release
- Faster, leaner packages: RPMs no longer ship the unused
  `-debuginfo`/`-debugsource` subpackages (the release binaries are already
  stripped), and the containerized rpm and Flatpak builds now cache cargo
  artifacts across releases so dependency crates aren't recompiled every time.

## 0.1.0

### Build & release
- GitHub Actions **CI** (`cargo fmt --check`, `clippy -D warnings`, tests) and a
  tag-triggered **Release** pipeline that builds `.deb`, `.rpm`, an Arch package,
  and Flatpak bundles (x86_64 + arm64; Arch x86_64-only) and attaches them to a
  GitHub Release.
- Self-hosted, GPG-signed **apt** and **flatpak** repos published to `gh-pages`
  so `.deb` and Flatpak installs auto-update. Packaging lives in `packaging/`;
  release process in `docs/RELEASING.md`. Added a top-level `LICENSE` (MIT).

### Session restore
- The viewer reports what it's showing (article + light/web mode) to the daemon
  over IPC; on the next plain open (app menu / tray) the daemon reopens that
  article in the same mode. Kept in daemon memory only (resets on daemon
  restart); no footprint cost — nothing is kept resident.

### M6 — Desktop integration
- App icon set (scalable SVG + 16–512 px PNGs, hicolor tree) and a desktop
  entry so **Fodder Reader** appears in the app menu with a proper icon/name.
- Per-user `install.sh` / `uninstall.sh` (binaries → `~/.local/bin`, icons +
  `.desktop` → `~/.local/share`), refreshing the icon/desktop caches.
- Wired the branded icon into the tray, desktop notifications (with a
  `desktop-entry` hint), the window/menu, and the autostart entry; window↔app
  matching via `StartupWMClass`. App ID centralized as `fodder_core::APP_ID`.
- Cross-desktop test checklist (`docs/cross-desktop-testing.md`) covering
  GNOME / KDE / XFCE / Sway, including the vanilla-GNOME tray degrade path.
- Launching from the app menu now starts the **daemon** (tray + polling) and
  opens the viewer: the desktop entry runs `fodderd --open-viewer` (a new flag
  that spawns the viewer once the daemon is up), while the autostart entry stays
  headless (`fodderd`). If a daemon is already running, it just opens the viewer.

### Notifications & live settings
- Notification preferences: a master **Enable notifications** switch gating
  **New articles** (the per-feed poll notifications) and a **Daily reading
  reminder** at a chosen local time (hour/minute), which fires once a day only
  when there are unread articles and the viewer is closed.
- Settings now apply **live**: the viewer sends a `ReloadConfig` IPC message on
  every change and the daemon re-reads its config immediately (no restart) —
  this also makes the poll-interval change take effect at once.

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
- Right-click context menus: on a feed — refresh this feed, mark all read,
  copy feed URL, remove feed; on an article — mark read/unread, open in
  browser, copy link.
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
