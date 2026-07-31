# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Git workflow - IMPORTANT

**Never push directly to `main`.** Always work on a feature branch and open a pull request so the GitHub Actions CI pipeline (fmt + clippy + tests) can run before merging.

1. Create a branch from the latest `main`:
   ```bash
   git checkout main && git pull
   git checkout -b <descriptive-branch-name>
   ```
2. Commit changes on the branch.
3. Push the branch and open a PR targeting `main`:
   ```bash
   git push -u origin <descriptive-branch-name>
   gh pr create --base main --title "..." --body "..."
   ```
4. Wait for CI to pass.
5. **Never merge a PR - merging is always the user's decision and action**, even when CI
   is green and all review comments are addressed. Stop when the PR is ready and report
   its URL.
6. After the user merges, a release is cut **automatically**: `auto-release.yml`
   (on push to `main`) reads the merged PR's `release:*` label for the bump size
   (default patch; `release:skip` opts out), bumps `Cargo.toml`/`Cargo.lock`,
   stamps `CHANGELOG.md`, tags `vX.Y.Z`, and hands off to `release.yml`. It
   **skips tooling/docs-only merges** — a release fires only when the app crates
   (`core/`/`fodderd/`/`fodder/`), assets (`data/`), packaging, or deps
   (`Cargo.toml`/`Cargo.lock`) change; a merge touching only `.github/`, `docs/`,
   `*.md`, or root scripts ships nothing and is skipped (a `release:*` bump label
   forces a release anyway). So label the PR `release:minor`/`release:major` when
   appropriate, or `release:skip` to merge without releasing. A manual `vX.Y.Z`
   tag push still works for off-cycle releases. See `docs/RELEASING.md`.

**One PR per prompt:** create exactly one pull request per user request, even when the
work is large. Use multiple commits on the same branch for reviewability instead of
fanning out into many small PRs - only split when the user explicitly asks.

This applies to all agents (Claude, Gemini, etc.) - no direct pushes to `main`, and no
merges, under any circumstances.

---

## Keep documentation in sync - IMPORTANT

Whenever a change affects user-facing behavior, features, architecture, commands, conventions, or test boundaries, update the relevant docs **in the same PR** so they never drift from the code:

- `README.md` - user-facing features, install, and usage
- `CHANGELOG.md` - move the relevant milestone/feature into a shipped section
- `CLAUDE.md` and `GEMINI.md` - architecture, commands, conventions, and test-coverage boundaries

Before opening a PR, re-read these files and reconcile anything the change made inaccurate (new modules, renamed flows, new settings, new IPC messages, new tests, changed defaults). Treat doc updates as part of "done," not a follow-up.

---

## Keep tests meaningful - IMPORTANT

For every change, add or update tests when doing so is meaningful - treat it as part of "done," not a follow-up. "Meaningful" means the test would actually catch a regression in the behavior you changed:

- New or changed logic with a testable contract (parsing, decisions, data transforms, DB queries, HTTP request/response handling) -> add or update unit tests covering the new behavior and its edge cases.
- Fixing a bug -> add a test that fails without the fix, so it can't silently regress.
- When the meaningful logic is tangled with hard-to-test platform code (GTK4 widgets, the tokio<->glib bridge), **extract the pure logic into a standalone function in `core` and test that** - e.g. feed discovery/parsing, conditional-GET classification, dedupe hashing, IPC framing, and the HTML->Pango reader all live as pure functions with unit tests, while the GTK UI in `fodder/` is not unit-tested.
- Run the suite before opening a PR: `cargo test --workspace` (plus `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`, which CI enforces).

Skip new tests only when a change genuinely has no testable behavior (docs, comments, pure formatting, workflow YAML, trivial constant tweaks) - and say so briefly rather than silently omitting them.

---

## What this repo is

A Cargo workspace (Rust, edition 2021) for **Fodder**, a lightweight RSS/Atom/JSON-Feed reader for Linux desktops. Three crates:

- `core/` (`fodder-core`) - shared library: models, config, SQLite store + migrations, HTTP poller, feed discovery, IPC protocol, autostart, XDG paths. All the pure/testable logic lives here.
- `fodderd/` - the **headless daemon** (tokio, no GTK): poll loop, system-tray icon, desktop notifications, the shared SQLite writer, and the single-instance IPC socket. It spawns the viewer on demand.
- `fodder/` - the **GTK4 + libadwaita viewer**: a 3-pane reader spawned by the daemon and freed on close.

**Process model.** `fodderd` is the primary, always-resident process. The `fodder` viewer is spawned on demand (tray click, notification action, app-menu launch, or a re-run) and terminated on close so its memory is freed while the daemon and tray stay resident. Exactly one daemon and one viewer are enforced via a Unix socket in `$XDG_RUNTIME_DIR/fodder/daemon.sock`, which doubles as the daemon<->viewer IPC channel. App ID: `io.github.dipakmdhrm.Fodder` (`fodder_core::APP_ID`).

**Storage:** config `~/.config/fodder/config.toml` (TOML), database `~/.local/share/fodder/db.sqlite` (SQLite, WAL). Paths resolved via `directories` in `core/src/paths.rs`.

---

## Commands

```bash
# Build / test the whole workspace
cargo build --workspace
cargo test --workspace

# Single crate / test
cargo test -p fodder-core
cargo test -p fodder-core --test conditional_get      # a specific integration test file

# Lint + format (CI enforces both)
cargo fmt --all
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Run the daemon (foreground, with logs)
RUST_LOG=info cargo run -p fodderd

# Run the viewer directly (it connects back to the daemon; standalone if none)
cargo run -p fodder

# Example tools for driving the running daemon / core logic
cargo run -p fodderd --example ctl -- ping | open | refresh | list | rm <id> \
                                      | subscribe <url> <title> | autostart on|off|status | reload
cargo run -p fodder-core --example poll -- <url>       # exercise discovery + a real conditional GET
```

**Dev gotcha:** `cargo run -p fodderd` only builds the daemon, but the daemon spawns the
viewer by launching the **`target/debug/fodder`** binary by path. After changing viewer
code, run `cargo build --workspace` (or `./install.sh`) first, or the daemon will spawn a
**stale** viewer.

**Packaging / release** lives in `packaging/` and `.github/workflows/`; see `docs/RELEASING.md`. The four package builds (`.deb`, `.rpm`, Arch, Flatpak) live in one reusable workflow, `build-packages.yml` (which takes a `version` and an optional `ref` to check out), that both `ci.yml` (on every PR, so a tag is just a repeat of an already-green build) and `release.yml` (on a `v*` tag **or** `workflow_call`, which then signs + publishes the apt/flatpak repos and cuts the GitHub Release) call. Releases are cut **automatically on merge to `main`** by `auto-release.yml`: it derives the next version from the newest tag and the merged PR's `release:*` label, bumps `Cargo.toml`/`Cargo.lock`, stamps `CHANGELOG.md`, commits + tags, and invokes `release.yml` pointed at the new tag. No release loop, because it pushes with `GITHUB_TOKEN` (whose pushes don't retrigger workflows) and calls `release.yml` via `workflow_call` rather than the token-pushed tag. The Arch build links the **system** SQLite (its PKGBUILD drops rusqlite's `bundled` feature) because makepkg's hardening link flags break the vendored static SQLite; the deb/rpm/flatpak/dev builds still bundle it. Per-user install without packaging: `./install.sh` / `./uninstall.sh [--purge]`.

---

## Architecture

### `core` (`fodder-core`)

- **`models.rs`** - `Feed`, `Article`, `NewArticle`. Timestamps are `chrono::DateTime<Utc>`.
- **`paths.rs`** - XDG config/data/runtime paths and the autostart `.desktop` path.
- **`config.rs`** - the TOML `Config`: `poll_interval_minutes` (validated >= 5), `poll_concurrency`, and the notification settings (`notifications_enabled`, `notify_new_articles`, `daily_reminder_enabled`, `daily_reminder_time` as `"HH:MM"`, parsed by `reminder_hm()`). `load()` clamps/normalizes bad on-disk values; `save()` writes atomically (temp + rename).
- **`db/`** - the SQLite store. `mod.rs` opens the connection with `WAL` + `foreign_keys=ON` + `busy_timeout` (so daemon and viewer can share the file), and holds a `PRAGMA user_version`-driven migration runner (`migrations.rs`) plus RFC3339 timestamp helpers. `feeds.rs`/`articles.rs` are free functions taking `&Connection`. **Dedupe is `INSERT OR IGNORE` on `UNIQUE(feed_id, guid)`**, returning only the genuinely-new row ids so seen items never re-notify. `reschedule()` preserves ETag/Last-Modified for 304/rate-limit outcomes.
- **`poller/`** - `Poller` owns a shared rustls `reqwest::Client`. `http.rs::conditional_get()` replays stored `ETag`/`Last-Modified` as `If-None-Match`/`If-Modified-Since` and classifies the response (304 -> `NotModified`, 429/503 + `Retry-After` -> `RateLimited`, 2xx -> body, else `Error`). `mod.rs` parses with `feed-rs`, computes bounded-concurrency `poll_all()` via `buffer_unordered`, and `backoff_next()` is capped exponential backoff. `dedupe.rs::stable_guid()` uses the entry id or a SHA-256 of link+title.
- **`discovery.rs`** - `resolve_feed()` tries to parse the fetched URL as a feed directly, else extracts `<link rel="alternate">` feed links via `scraper` (`extract_feed_links()` is the pure, tested helper), resolving relative hrefs. Returns `DirectFeed | Candidates | None`.
- **`ipc.rs`** - the `IpcMessage` enum and length-prefixed JSON framing (`u32` big-endian length + `serde_json`) over any `AsyncRead`/`AsyncWrite`. This is the daemon<->viewer wire protocol.
- **`autostart.rs`** - writes/removes `~/.config/autostart/fodder.desktop` (launching `fodderd`); shared by the daemon and the viewer's settings.

### `fodderd` (daemon)

**Entry** (`main.rs`): parses `--open-viewer` (used by the app-menu launcher - opens the viewer once the daemon is up; without it, e.g. autostart, the daemon stays headless), acquires the single-instance socket, opens/migrates the DB, builds `AppCtx`, and spawns the long-running tasks.

- **`state.rs`** - `AppCtx`: the shared `Arc<Mutex<Db>>` (touched only inside `spawn_blocking` via `with_conn`), the `Poller`, live `Arc<RwLock<Config>>`, the viewer's outbound channel + liveness flags, open/refresh channels, and the in-memory `ReadingState` (last-read article + light/web mode, for session restore). `ReloadConfig` re-reads config from disk into the `RwLock`.
- **`single_instance.rs`** - binds the socket; on `AddrInUse` it probes with `Ping`/`Pong` to tell a live daemon from a stale post-crash file (which it unlinks and rebinds). A second launch sends `OpenViewer` and exits.
- **`server.rs`** - the IPC accept loop and dispatch. Each connection gets a writer task draining an outbound queue. Handles `Ping`, `ViewerHello` (registers the one viewer; rejects a duplicate and raises the existing one), `RefreshNow`, `SubscribeResolved` (insert + immediate poll + `FeedsChanged`), `ReloadConfig`, `ReadingState`, and `OpenViewer`/`OpenAt`.
- **`scheduler.rs`** - the poll loop: sleeps until the soonest `next_poll_at` (or a refresh signal via `tokio::select!`), polls due feeds concurrently, and stores each outcome (Updated -> insert + success + validators; NotModified/RateLimited -> `reschedule` preserving validators; Error -> `update_feed_error` with backoff). New-article notifications are gated on the live config. A feed's title is filled from the feed document only when it's empty (so a discovery/subscribe-time title isn't clobbered).
- **`notify.rs`** - batched, actionable `notify-rust` notifications (one per feed for new articles; plus the daily reminder). The blocking `wait_for_action` runs on its own OS thread so it never ties up tokio; a click routes an `OpenRequest`.
- **`reminder.rs`** - the daily reading-reminder task: fires at the local `HH:MM` only when enabled, there are unread articles, **and the viewer is closed**; reschedules daily and recomputes on `reminder_reload`.
- **`tray.rs`** - the `ksni` StatusNotifierItem tray (Open / Refresh all / Quit; left-click opens), run under a **self-heal supervisor** (`supervise()`). Registration is **best-effort** - on hosts without an SNI tray (e.g. vanilla GNOME Shell) it logs and degrades, and the daemon keeps polling/notifying. The icon is our app PNG **embedded and decoded into ARGB `IconPixmap`s** with an empty `IconName` (several hosts, notably the GNOME AppIndicator extension, mishandle a themed `IconName` and show a placeholder instead of falling back to the pixmap). `ksni` re-registers when the watcher restarts on the same bus, but that event-driven recovery can still miss a drop (observed live on GNOME: daemon running and owning its item name, yet absent from the watcher's registered list). So the supervisor periodically (30s) asks the `org.kde.StatusNotifierWatcher` for its `RegisteredStatusNotifierItems`, resolves each entry's owner PID, and if none is ours re-registers by re-spawning the tray - cause-agnostic, catching prunes that don't emit a watcher `NameOwnerChanged`. Uses `zbus` for the query and `png` to decode the embedded icon.
- **`viewer_proc.rs`** - on-demand viewer process management: spawns the single `fodder` child on an open request (navigation target + `--webkit` passed as CLI args, restored from `ReadingState` on a plain "Show"), reaps it on exit, and kills it on daemon shutdown.
- **`self_update.rs`** - self-restart on in-place binary replacement (release builds only): polls the installed `fodderd`'s file signature and, when a package upgrade swaps it out, sets a re-exec flag and triggers shutdown; `main` then tears down socket/tray/viewer and `exec`s the new binary in the same session, so the tray survives `apt upgrade` instead of vanishing until next login. The deb `prerm` accordingly no longer kills the daemon on upgrade (rpm/Arch scripts already skip upgrades).

### `fodder` (viewer)

- **`main.rs`** - parses `--feed`/`--article`/`--webkit` into a `Target`, then runs a NON_UNIQUE `adw::Application` (the daemon arbitrates single-instance, not GApplication).
- **`runtime.rs`** - the tokio<->glib bridge: `run_db()` runs blocking SQLite on the tokio pool and applies the result on the GTK main thread via `glib::spawn_future_local`; `run_async()` does the same for arbitrary async (HTTP discovery). Never touch GTK widgets off the main thread.
- **`ipc_client.rs`** - runs on the tokio runtime: connects, sends `ViewerHello`, forwards inbound daemon messages to the GTK thread as `FromDaemon` events, and writes outbound commands (refresh, subscribe, `ReadingState` reports).
- **`app.rs`** - the whole 3-pane UI held in an `Rc<App>`: `OverlaySplitView` (feeds sidebar) + `NavigationSplitView` (article list | reader). Feeds show unread counts (bold); the article list marks-read-on-open; the reader has the light renderer + the WebKit toggle. Right-click context menus (popovers parented to the *panes*, not the lists - parenting inside the `ScrolledWindow` clamps their height). Programmatic row selections must be wrapped in `suppress` so they don't re-fire the `row-selected` handlers. Reports `ReadingState` on article open + mode toggle; restores mode via `pending_webkit`.
- **`reader.rs`** - the light reader: article HTML -> `ammonia` sanitize -> a whitelist walk (`scraper`/`ego-tree`) -> the limited set of tags Pango understands. No scripts, no network.

**WebKit reader:** the full-view toggle loads the **live article URL** (`load_uri`), JS off, images on, an ephemeral `NetworkSession` with tracking prevention, and a **dedicated `WebContext` per view**. On toggle-back/article-change, `destroy_webview()` calls `terminate_web_process()` and drops the view + its context so the `WebKitWebProcess`/`WebKitNetworkProcess` actually exit (WebKit's shared default context otherwise pools them and ~300 MB never releases).

---

## Test coverage boundaries

Tests live next to the code in `core/` (plus one integration file), and the daemon has a couple of pure-logic tests. GTK UI is not unit-tested; pure logic is extracted into `core`.

- **`core/src/config.rs`** - default validity, >=5-minute clamping on load, save/load round-trip, and `reminder_hm()` parsing (valid/invalid, reset-on-bad-value).
- **`core/src/db/feeds.rs`** - insert/get/list, duplicate-URL rejection, success clearing error + storing validators, `feeds_due` time filtering, and `reschedule` preserving validators.
- **`core/src/db/articles.rs`** - `INSERT OR IGNORE` dedupe (second insert returns no ids), cascade delete, unread counts + mark-read/mark-all, mark-read<->unread toggle, and listing.
- **`core/src/db/migrations.rs`** - migrations apply and are idempotent.
- **`core/src/discovery.rs`** - multi-candidate extraction with relative-href resolution, JSON-feed recognition, ignoring non-feed links, and MIME-with-charset matching (all on fixture HTML, no network).
- **`core/src/poller/dedupe.rs`** - GUID-when-present, SHA-256 link+title fallback (stable/deterministic/idempotent).
- **`core/src/poller/http.rs`** - `parse_retry_after` (seconds, HTTP-date, garbage).
- **`core/src/poller/mod.rs`** - RSS parse into title + items; `backoff_next` growth and cap.
- **`core/src/ipc.rs`** - round-trip of every `IpcMessage` variant over `tokio::io::duplex`, partial-frame reassembly, and clean-EOF -> `None`.
- **`core/tests/conditional_get.rs`** - the conditional-GET path against a `wiremock` server: conditional headers actually sent (verified via the recorded request), 304 handling, validator capture, 429 with seconds and HTTP-date, and non-2xx -> error.
- **`fodder/src/reader.rs`** - HTML->Pango: scripts stripped, basic formatting converted, entities escaped, safe links kept.
- **`fodderd/src/reminder.rs`** - the next-occurrence time math stays within a day.
- **`fodderd/src/self_update.rs`** - the binary-signature comparison detects an in-place replacement (and a missing path reads as `None`); the watch loop and `exec` handoff are platform glue, not unit-tested.
- **`fodderd/src/tray.rs`** - the pure tray helpers: `bus_name_of` (strip the `@objectpath` suffix from a registered-item entry), `rgba_to_argb`/`rgb_to_argb` channel reordering, and that the embedded PNGs actually decode to correctly-sized pixmaps. The `ksni` wiring, the D-Bus registration query, and the re-spawn loop are platform glue, not unit-tested (exercised manually via the running daemon + `gdbus`).

The GTK4 widget code in `fodder/src/app.rs`, the tokio<->glib bridge, and the daemon's async task wiring are **not** unit-tested; the daemon's IPC/lifecycle behavior is exercised manually via the `ctl` example and isolated shell runs.
