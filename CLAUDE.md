# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Git workflow - IMPORTANT

**Never push directly to `main`.** Always work on a feature branch and open a pull request so the GitHub Actions CI pipeline (Rust fmt + clippy + tests, and the Android lint/test/debug-build job) can run before merging.

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
6. After the user merges, a release is cut **automatically** for whichever
   platform the merge touched. `auto-release.yml` (on push to `main`) reads the
   merged PR's `release:*` label for the bump size (default patch;
   `release:skip` opts out), then:
   - a merge touching `linux/` bumps `linux/Cargo.toml`/`linux/Cargo.lock`,
     stamps `CHANGELOG.md`, tags `vX.Y.Z`, and hands off to `release.yml`;
   - a merge touching `android/` tags `android-X.Y.Z` and hands off to
     `release-android.yml` (nothing to bump — the APK's
     `versionName`/`versionCode` come from the tag);
   - a merge touching both cuts both.

   A merge touching only `.github/`, `docs/`, `*.md`, or root files ships
   nothing and is skipped (an explicit `release:major`/`release:minor` label
   forces a **Linux** release anyway). So label the PR
   `release:minor`/`release:major` when appropriate, or `release:skip` to merge
   without releasing. Manual `vX.Y.Z` / `android-X.Y.Z` tag pushes still work
   for off-cycle releases. See `docs/RELEASING.md` and `docs/ANDROID.md`.

**One PR per prompt:** create exactly one pull request per user request, even when the
work is large. Use multiple commits on the same branch for reviewability instead of
fanning out into many small PRs - only split when the user explicitly asks.

This applies to all agents (Claude, Gemini, etc.) - no direct pushes to `main`, and no
merges, under any circumstances.

---

## Keep documentation in sync - IMPORTANT

Whenever a change affects user-facing behavior, features, architecture, commands, conventions, or test boundaries, update the relevant docs **in the same PR** so they never drift from the code:

- `README.md` - user-facing features, install, and usage, for **both** platforms
- `CHANGELOG.md` - move the relevant milestone/feature into a shipped section
- `CLAUDE.md` and `GEMINI.md` - architecture, commands, conventions, and test-coverage boundaries

Before opening a PR, re-read these files and reconcile anything the change made inaccurate (new modules, renamed flows, new settings, new IPC messages, new tests, changed defaults). Treat doc updates as part of "done," not a follow-up.

---

## Keep tests meaningful - IMPORTANT

For every change, add or update tests when doing so is meaningful - treat it as part of "done," not a follow-up. "Meaningful" means the test would actually catch a regression in the behavior you changed:

- New or changed logic with a testable contract (parsing, decisions, data transforms, DB queries, HTTP request/response handling) -> add or update unit tests covering the new behavior and its edge cases.
- Fixing a bug -> add a test that fails without the fix, so it can't silently regress.
- When the meaningful logic is tangled with hard-to-test platform code (GTK4 widgets, the tokio<->glib bridge, Android ViewModels/Compose), **extract the pure logic into a standalone function in `core` (or, on Android, a plain class/top-level function) and test that** - e.g. feed discovery/parsing, conditional-GET classification, dedupe hashing, IPC framing, and the HTML->Pango reader all live as pure functions with unit tests, while the GTK UI in `fodder/` is not unit-tested.
- Run the suite before opening a PR: in `linux/`, `cargo test --workspace` (plus `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`); in `android/`, `./gradlew ktlintCheck testDebugUnitTest`. CI enforces all of them.
- **Porting rule:** when you change a rule that both platforms implement (feed parsing, dedupe, conditional-GET classification, backoff, the refresh summary, HTML sanitizing), change it on both sides in the same PR and update both tests. Each Kotlin port names its Rust counterpart in a KDoc comment — grep for `core/src/` in `android/` to find them.

Skip new tests only when a change genuinely has no testable behavior (docs, comments, pure formatting, workflow YAML, trivial constant tweaks) - and say so briefly rather than silently omitting them.

---

## What this repo is

Two independent apps for **Fodder**, a lightweight RSS/Atom/JSON-Feed reader, in one repo:

```
linux/     the Rust Cargo workspace, packaging, desktop assets, install scripts
android/   the Kotlin + Jetpack Compose app (Gradle)
docs/ .github/ README.md CHANGELOG.md   shared
```

They share a design and a set of behavioral rules, **not code**: each keeps its own subscriptions and database, and there is no sync between them. The Android app deliberately reimplements the pure logic that `linux/core` has in Rust; every ported Kotlin file names its Rust counterpart, and both sides carry equivalent tests. Releases are independent (`vX.Y.Z` vs `android-X.Y.Z`).

### `linux/` - a Cargo workspace (Rust, edition 2021) with three crates:

- `core/` (`fodder-core`) - shared library: models, config, SQLite store + migrations, HTTP poller, feed discovery, IPC protocol, autostart, XDG paths. All the pure/testable logic lives here.
- `fodderd/` - the **headless daemon** (tokio, no GTK): poll loop, system-tray icon, desktop notifications, the shared SQLite writer, and the single-instance IPC socket. It spawns the viewer on demand.
- `fodder/` - the **GTK4 + libadwaita viewer**: a 3-pane reader spawned by the daemon and freed on close.

**Process model.** `fodderd` is the primary daemon: resident **for the graphical session** (it exits on logout when the D-Bus session bus goes away, and the next login's autostart brings it back fresh - see `tray.rs`). The `fodder` viewer is spawned on demand (tray click, notification action, app-menu launch, or a re-run) and, by default, kept resident on close: the viewer's `close_request` handler hides the window so reopening is an instant `present()` instead of a cold spawn. The live WebView is intentionally kept too (not `destroy_webview()`d on hide), so returning to a full-view article is instant - the resident process holds WebKit's subprocesses, the memory cost this default accepts. The `low_memory_mode` config opts back into teardown - when enabled, `close_request` proceeds and the process exits on close, freeing its memory (~150 MB, mostly the private GTK4/adwaita heap) at the cost of a cold reopen. Default off. The decision lives entirely viewer-side (a live `App::low_memory` cell, updated the moment the preference toggles); the daemon is unchanged (a resident viewer simply keeps answering `OpenViewer`). Exactly one daemon and one viewer are enforced via a Unix socket in `$XDG_RUNTIME_DIR/fodder/daemon.sock`, which doubles as the daemon<->viewer IPC channel. App ID: `io.github.dipakmdhrm.Fodder` (`fodder_core::APP_ID`).

**Storage:** config `~/.config/fodder/config.toml` (TOML), database `~/.local/share/fodder/db.sqlite` (SQLite, WAL). Paths resolved via `directories` in `core/src/paths.rs`.

### `android/` - a Gradle project, `minSdk` 26 / `compileSdk` 35

A single `:app` module, Kotlin + Compose + Material 3, Room for storage, OkHttp for fetching, WorkManager for the background poll. No daemon, no tray, no IPC: one process owns everything. Application id `io.github.dipakmdhrm.fodder`. Versions come from the git tag at build time, so there is no version file to bump.

---

## Commands

All Rust commands run from `linux/`, all Gradle commands from `android/`.

```bash
# --- linux/ ---------------------------------------------------------------
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

# --- android/ -------------------------------------------------------------
./gradlew testDebugUnitTest        # JVM unit tests (Robolectric for the DAO tests)
./gradlew ktlintCheck              # lint; ktlintFormat fixes most findings
./gradlew jacocoDebugUnitTestReport  # coverage, report-only (CI uploads it)
./gradlew assembleDebug            # app/build/outputs/apk/debug/app-debug.apk
adb install -r app/build/outputs/apk/debug/app-debug.apk
```

**Dev gotcha:** `cargo run -p fodderd` only builds the daemon, but the daemon spawns the
viewer by launching the **`target/debug/fodder`** binary by path. After changing viewer
code, run `cargo build --workspace` (or `./install.sh`) first, or the daemon will spawn a
**stale** viewer.

**Packaging / release** lives in `linux/packaging/` and `.github/workflows/`; see `docs/RELEASING.md`. The four package builds (`.deb`, `.rpm`, Arch, Flatpak) live in one reusable workflow, `build-packages.yml` (which takes a `version` and an optional `ref` to check out), that both `ci.yml` (on every PR, so a tag is just a repeat of an already-green build) and `release.yml` (on a `v*` tag **or** `workflow_call`, which then signs + publishes the apt/flatpak repos and cuts the GitHub Release) call. Releases are cut **automatically on merge to `main`** by `auto-release.yml`: it derives the next version from the newest tag and the merged PR's `release:*` label, bumps `linux/Cargo.toml`/`linux/Cargo.lock`, stamps `CHANGELOG.md`, commits + tags, and invokes `release.yml` pointed at the new tag; a merge under `android/` instead pushes an `android-X.Y.Z` tag and invokes `release-android.yml`, which decodes the signing keystore from repository secrets, builds a signed APK, and attaches it to a GitHub Release (see `docs/ANDROID.md`). No release loop, because it pushes with `GITHUB_TOKEN` (whose pushes don't retrigger workflows) and calls `release.yml` via `workflow_call` rather than the token-pushed tag. The Arch build links the **system** SQLite (its PKGBUILD drops rusqlite's `bundled` feature) because makepkg's hardening link flags break the vendored static SQLite; the deb/rpm/flatpak/dev builds still bundle it. Per-user install without packaging: `linux/install.sh` / `linux/uninstall.sh [--purge]`. `ci.yml` also runs an Android job (ktlint + JVM tests + `assembleDebug`) on every PR; both jobs are unconditional so a required check is never stuck "expected" on a single-platform PR. Once the app is live on Flathub, `flathub-publish.yml` (invoked by `release.yml`, so it fires for both auto-releases and manual tags) regenerates `cargo-sources.json`, repoints the Flathub manifest at the new tag+commit, and opens an auto-merging update PR to `flathub/io.github.dipakmdhrm.Fodder` — dormant until the repo variable `FLATHUB_AUTOPUBLISH=true` and the `FLATHUB_TOKEN` secret are set (see `docs/RELEASING.md`).

**Flathub submission** artifacts live in `linux/packaging/flatpak/flathub/` (separate from the self-hosted `linux/packaging/flatpak/` manifest, which curls rustup and builds online). Both build from the `linux/` workspace: the self-hosted one because its `path: ../..` source now resolves there, the Flathub one via `subdir: linux` on its module (`CARGO_HOME` is absolute, so the vendored crate set still resolves from the source root). The Flathub manifest builds **fully offline** as Flathub's workers require: Rust comes from the `org.freedesktop.Sdk.Extension.rust-stable` SDK extension (no rustup), and every crate is vendored via `cargo-sources.json` (regenerate with `flatpak-cargo-generator.py linux/Cargo.lock` whenever deps change) with `CARGO_NET_OFFLINE=true`. Sources are pinned to a git **tag + commit** — the current pin predates the `linux/` split, so it must be moved to a tag that contains the new layout before any submission. The AppStream metainfo is `linux/data/metainfo/io.github.dipakmdhrm.Fodder.metainfo.xml` (validate with `appstreamcli validate`); its `<release>` list and the screenshot URL must be kept current. Lint the manifest/repo with `flatpak-builder-lint` (portals need **no** `--talk-name`; `fallback-x11` needs `--share=ipc`).

---

## Architecture (Linux)

### `core` (`fodder-core`)

- **`models.rs`** - `Feed`, `Article`, `NewArticle`. Timestamps are `chrono::DateTime<Utc>`.
- **`paths.rs`** - XDG config/data/runtime paths and the autostart `.desktop` path.
- **`config.rs`** - the TOML `Config`: `poll_interval_minutes` (validated >= 5), `poll_concurrency`, `low_memory_mode` (default false; frees the viewer on close instead of keeping it resident), and the notification settings (`notifications_enabled`, `notify_new_articles`, `daily_reminder_enabled`, `daily_reminder_time` as `"HH:MM"`, parsed by `reminder_hm()`). `load()` clamps/normalizes bad on-disk values; `save()` writes atomically (temp + rename).
- **`db/`** - the SQLite store. `mod.rs` opens the connection with `WAL` + `foreign_keys=ON` + `busy_timeout` (so daemon and viewer can share the file), and holds a `PRAGMA user_version`-driven migration runner (`migrations.rs`) plus RFC3339 timestamp helpers. `feeds.rs`/`articles.rs` are free functions taking `&Connection`. **Dedupe is `INSERT OR IGNORE` on `UNIQUE(feed_id, guid)`**, returning only the genuinely-new row ids so seen items never re-notify. `reschedule()` preserves ETag/Last-Modified for 304/rate-limit outcomes.
- **`poller/`** - `Poller` owns a shared rustls `reqwest::Client`. `http.rs::conditional_get()` replays stored `ETag`/`Last-Modified` as `If-None-Match`/`If-Modified-Since` and classifies the response (304 -> `NotModified`, 429/503 + `Retry-After` -> `RateLimited`, 2xx -> body, else `Error`). `mod.rs` parses with `feed-rs`, computes bounded-concurrency `poll_all()` via `buffer_unordered`, and `backoff_next()` is capped exponential backoff. `dedupe.rs::stable_guid()` uses the entry id or a SHA-256 of link+title.
- **`discovery.rs`** - `resolve_feed()` tries to parse the fetched URL as a feed directly, else extracts `<link rel="alternate">` feed links via `scraper` (`extract_feed_links()` is the pure, tested helper), resolving relative hrefs. Returns `DirectFeed | Candidates | None`.
- **`ipc.rs`** - the `IpcMessage` enum and length-prefixed JSON framing (`u32` big-endian length + `serde_json`) over any `AsyncRead`/`AsyncWrite`. This is the daemon<->viewer wire protocol.
- **`autostart.rs`** - launch-at-login. Natively writes/removes `~/.config/autostart/fodder.desktop` (launching `fodderd`); shared by the daemon and the viewer's settings. **Inside Flatpak** that private path is useless, so autostart goes through the XDG **Background** portal instead: `is_flatpak()` detects the sandbox, `portal_autostart_command()` is the pure command the portal registers (start `fodderd` headless), and a marker file (`flatpak_marker_path()`) records intent since the portal has no getter. The async portal call itself lives in `fodderd/portal.rs`; the viewer routes its toggle to the daemon via the `SetAutostart` IPC message.

### `fodderd` (daemon)

**Entry** (`main.rs`): parses `--open-viewer` (used by the app-menu launcher - opens the viewer once the daemon is up; without it, e.g. autostart, the daemon stays headless), acquires the single-instance socket, opens/migrates the DB, builds `AppCtx`, and spawns the long-running tasks.

- **`state.rs`** - `AppCtx`: the shared `Arc<Mutex<Db>>` (touched only inside `spawn_blocking` via `with_conn`), the `Poller`, live `Arc<RwLock<Config>>`, the viewer's outbound channel + liveness flags, open/refresh channels, and the in-memory `ReadingState` (last-read article + light/web mode, for session restore). `ReloadConfig` re-reads config from disk into the `RwLock`.
- **`single_instance.rs`** - binds the socket; on `AddrInUse` it probes with `Ping`/`Pong` to tell a live daemon from a stale post-crash file (which it unlinks and rebinds). A second launch sends `OpenViewer` and exits.
- **`server.rs`** - the IPC accept loop and dispatch. Each connection gets a writer task draining an outbound queue. Handles `Ping`, `ViewerHello` (registers the one viewer; rejects a duplicate and raises the existing one), `RefreshNow` (forwarded to the scheduler, which then emits the `RefreshStarted`/`RefreshFinished` progress messages), `SubscribeResolved` (insert + immediate poll + `FeedsChanged`), `RenameFeed` (update title + `FeedsChanged`), `ReloadConfig`, `ReadingState`, and `OpenViewer`/`OpenAt`.
- **`scheduler.rs`** - the poll loop: sleeps until the soonest `next_poll_at` (or a refresh signal via `tokio::select!`), polls due feeds concurrently, and stores each outcome (Updated -> insert + success + validators; NotModified/RateLimited -> `reschedule` preserving validators; Error -> `update_feed_error` with backoff). `poll_feeds` returns a `PollSummary` (new-article + hard-error counts) and fires `FeedsChanged` when anything new landed. **User-invoked** refreshes (`poll_all_now`/`poll_one`, via `refresh_reporting`) bracket the poll with `RefreshStarted`/`RefreshFinished` IPC messages carrying the counts + wall-clock duration, so an open viewer can show progress + a completion toast; **scheduled** `poll_due` polls stay silent. New-article notifications are gated on the live config. A feed's title is filled from the feed document only when it's empty (so a discovery/subscribe-time title isn't clobbered).
- **`notify.rs`** - batched, actionable `notify-rust` notifications (one per feed for new articles; plus the daily reminder). The blocking `wait_for_action` runs on its own OS thread so it never ties up tokio; a click routes an `OpenRequest`.
- **`reminder.rs`** - the daily reading-reminder task: fires at the local `HH:MM` only when enabled, there are unread articles, **and the viewer is closed**; reschedules daily and recomputes on `reminder_reload`.
- **`tray.rs`** - the `ksni` StatusNotifierItem tray (Open / Refresh all / Quit; left-click opens), created by `spawn()` and owned by `main`. Registration is **best-effort** - on hosts without an SNI tray (e.g. vanilla GNOME Shell) it logs and degrades, and the daemon keeps polling/notifying. The icon is our app PNG **embedded and decoded into ARGB `IconPixmap`s** with an empty `IconName` (several hosts, notably the GNOME AppIndicator extension, mishandle a themed `IconName` and show a placeholder instead of falling back to the pixmap; `png` decodes the embedded PNGs once). The tray is kept simple by **tying it to the session lifecycle** rather than surviving across sessions: `spawn` passes `disable_dbus_name(true)` (register under `ksni`'s unique connection name, not a well-known `org.kde.StatusNotifierItem-<pid>-1` - works in Flatpak and never collides on restart) and `assume_sni_available(true)` (tolerate autostarting before the host is up; `ksni` registers when the watcher appears, and re-registers across watcher restarts). `wait_for_session_end()` awaits `zbus::Connection::closed()`; `main` uses it as a shutdown trigger, so when a **logout severs the D-Bus session connection** the daemon exits and the next login's autostart brings up a fresh one - no in-daemon re-registration/reconnection logic.
- **`viewer_proc.rs`** - on-demand viewer process management: spawns the single `fodder` child on an open request (navigation target + `--webkit` passed as CLI args, restored from `ReadingState` on a plain "Show"), reaps it on exit, and kills it on daemon shutdown.
- **`self_update.rs`** - self-restart on in-place binary replacement (release builds only): polls the installed `fodderd`'s file signature and, when a package upgrade swaps it out, sets a re-exec flag and triggers shutdown; `main` then tears down socket/tray/viewer and `exec`s the new binary in the same session, so the tray survives `apt upgrade` instead of vanishing until next login. The deb `prerm` accordingly no longer kills the daemon on upgrade (rpm/Arch scripts already skip upgrades).

### `fodder` (viewer)

- **`main.rs`** - parses `--feed`/`--article`/`--webkit` into a `Target`, then runs a NON_UNIQUE `adw::Application` (the daemon arbitrates single-instance, not GApplication). `--version`/`-V` short-circuits before GTK starts, printing `fodder_core::version_blurb("fodder")` and exiting; `fodderd` handles the same flag at the top of `main`.
- **`runtime.rs`** - the tokio<->glib bridge: `run_db()` runs blocking SQLite on the tokio pool and applies the result on the GTK main thread via `glib::spawn_future_local`; `run_async()` does the same for arbitrary async (HTTP discovery). Never touch GTK widgets off the main thread.
- **`ipc_client.rs`** - runs on the tokio runtime: connects, sends `ViewerHello`, forwards inbound daemon messages to the GTK thread as `FromDaemon` events, and writes outbound commands (refresh, subscribe, `ReadingState` reports).
- **`app.rs`** - the whole 3-pane UI held in an `Rc<App>`: `OverlaySplitView` (feeds sidebar) + `NavigationSplitView` (article list | reader). Feeds show unread counts (bold); the article list marks-read-on-open; the reader has the light renderer + the WebKit toggle. Right-click context menus (popovers parented to the *panes*, not the lists - parenting inside the `ScrolledWindow` clamps their height); the feed menu offers Refresh / Mark all as read / Rename / Delete / Copy feed URL, where Rename sends `RenameFeed` after a pre-filled title dialog. Programmatic row selections must be wrapped in `suppress` so they don't re-fire the `row-selected` handlers. Reports `ReadingState` on article open + mode toggle; restores mode via `pending_webkit`. The whole UI is wrapped in an `adw::ToastOverlay`: on `RefreshStarted` the header refresh button swaps to a spinner (disabled), and on `RefreshFinished` it restores and a toast reports the outcome via `fodder_core::refresh::format_refresh_summary`. The header-bar gear is a `MenuButton` whose menu (Preferences / About Fodder) resolves against an `appmenu` `SimpleActionGroup` on the window; **About** builds an `adw::AboutDialog` from the `fodder_core` app-metadata constants.
- **`reader.rs`** - the light reader: article HTML -> `ammonia` sanitize -> a whitelist walk (`scraper`/`ego-tree`) -> the limited set of tags Pango understands. No scripts, no network.

**WebKit reader:** the full-view toggle loads the **live article URL** (`load_uri`), JS off, images on, an ephemeral `NetworkSession` with tracking prevention, and a **dedicated `WebContext` per view**. On toggle-back/article-change, `destroy_webview()` calls `terminate_web_process()` and drops the view + its context so the `WebKitWebProcess`/`WebKitNetworkProcess` actually exit (WebKit's shared default context otherwise pools them and ~300 MB never releases).

---

---

## Architecture (Android)

One `:app` module, no daemon and no IPC - a single process owns the database, the poller, and the UI.

**Application + DI** (`FodderApp.kt`): `FodderApp` owns an `AppContainer` (manual dependency container - deliberately no Hilt/Koin at this size) holding the Room database, the `SettingsStore`, one shared `OkHttpClient` (with the `FodderReader/<version>` User-Agent the desktop also sends), and the `FeedRepository`. ViewModels get their dependencies through the shared `appViewModelFactory` (`viewModelFactory { initializer { ... } }` reading the app off `APPLICATION_KEY`) rather than casting `application`. `AppContainer.trackSettings()` keeps the poll interval and the notification toggle the poller reads in sync with DataStore, so a preference change takes effect without a restart.

**Storage** (`data/db/`): Room, one file `fodder.db`. `FeedEntity`/`ArticleEntity` mirror `core/src/models.rs`, with two differences that are deliberate: timestamps are epoch millis rather than RFC 3339 text, and migrations use Room's machinery rather than the desktop's `PRAGMA user_version` runner (which exists because a daemon and a viewer share that file; here one process owns it). **Dedupe is `OnConflictStrategy.IGNORE` against the unique `(feedId, guid)` index** - `insertAll` returns `-1` for rows that already existed, so callers count genuinely-new articles and a seen item never re-notifies. That is the exact role `INSERT OR IGNORE` plays on the desktop.

**Polling** (`data/FeedRepository.kt`): the Android counterpart of `fodderd/src/scheduler.rs`, and the same outcome handling - `Modified` inserts and stores fresh validators, `NotModified`/`RateLimited` reschedule **without** touching validators, `Error` records the message and backs off. A feed's title is filled from the feed document only when the local one is blank, so a rename is never clobbered. `now` and `pollSpacing` are constructor-injected so the workflow is exercisable without a clock or a settings store.

**Background refresh** (`work/PollWorker.kt`): a WorkManager `PeriodicWorkRequest` with a CONNECTED constraint, enqueued uniquely and updated (not restarted) when the interval setting changes. Two limits worth stating plainly and repeating to users: WorkManager will not run periodic work more often than **every 15 minutes** (the desktop config allows 5), and Doze can delay a run well past its window. Delivery is best-effort; pull-to-refresh is the way to force a poll. Notifications (`work/Notifier.kt`) are batched one per feed, matching `fodderd/src/notify.rs`, and a tap deep-links into the article through `MainActivity.EXTRA_ARTICLE_ID`.

**UI** (`ui/`): the desktop's three panes become three destinations (`ui/nav/Routes.kt`) - feeds, articles, reader - plus settings. The feed list bolds unread feeds and carries the same context menu as the desktop sidebar (Refresh / Mark all as read / Rename / Delete). The reader has the same two modes: the sanitized light render, or the live page in a WebView with JavaScript off and no DOM storage, which is the phone's version of the desktop's locked-down WebKit view.

**Ported pure logic.** Each of these names its Rust counterpart in a KDoc comment; change one and change the other in the same PR:

| Kotlin | Rust |
|--------|------|
| `data/feed/FeedParser.kt` | `core/src/poller/mod.rs::parse_items` (jsoup XML + kotlinx.serialization instead of `feed-rs`) |
| `data/feed/Dedupe.kt` | `core/src/poller/dedupe.rs` |
| `data/feed/Discovery.kt` | `core/src/discovery.rs` |
| `data/http/ConditionalGet.kt` | `core/src/poller/http.rs` |
| `data/http/Backoff.kt` | `core/src/poller/mod.rs::backoff_next` |
| `work/RefreshSummary.kt` | `core/src/refresh.rs` (hyphen separator instead of a middle dot) |
| `reader/HtmlToAnnotated.kt` | `fodder/src/reader.rs` (jsoup `Safelist` instead of `ammonia`) |

---

## Test coverage boundaries

### Linux

Tests live next to the code in `core/` (plus one integration file), and the daemon has a couple of pure-logic tests. GTK UI is not unit-tested; pure logic is extracted into `core`.

- **`core/src/lib.rs`** - `version_blurb()`: the first line is `<bin> <version>` and the body carries the app name, description, repository, and license (and honors the passed binary name).
- **`core/src/config.rs`** - default validity, >=5-minute clamping on load, save/load round-trip (incl. `low_memory_mode`), `low_memory_mode` defaulting off (both `Default` and an older config missing the field), and `reminder_hm()` parsing (valid/invalid, reset-on-bad-value).
- **`core/src/db/feeds.rs`** - insert/get/list, duplicate-URL rejection, success clearing error + storing validators, `update_feed_title` renaming, `feeds_due` time filtering, and `reschedule` preserving validators.
- **`core/src/db/articles.rs`** - `INSERT OR IGNORE` dedupe (second insert returns no ids), cascade delete, unread counts + mark-read/mark-all, mark-read<->unread toggle, and listing.
- **`core/src/db/migrations.rs`** - migrations apply and are idempotent.
- **`core/src/discovery.rs`** - multi-candidate extraction with relative-href resolution, JSON-feed recognition, ignoring non-feed links, and MIME-with-charset matching (all on fixture HTML, no network).
- **`core/src/poller/dedupe.rs`** - GUID-when-present, SHA-256 link+title fallback (stable/deterministic/idempotent).
- **`core/src/poller/http.rs`** - `parse_retry_after` (seconds, HTTP-date, garbage).
- **`core/src/poller/mod.rs`** - RSS parse into title + items; `backoff_next` growth and cap.
- **`core/src/ipc.rs`** - round-trip of every `IpcMessage` variant (incl. `SetAutostart`, `RenameFeed`, `RefreshStarted`/`RefreshFinished`) over `tokio::io::duplex`, partial-frame reassembly, and clean-EOF -> `None`.
- **`core/src/refresh.rs`** - `format_refresh_summary()`: the completion-toast text — "Up to date" vs singular/plural new-article counts, optional error clause, and the `Ns` / `NmSSs` duration formatting.
- **`core/src/autostart.rs`** - the pure helpers: `portal_autostart_command()` starts the daemon headless (no viewer), the marker path resolves under the config dir, and `resolve_enabled()` reads the desktop file natively vs the marker under Flatpak (pinning that `.deb`/`.rpm`/Arch behavior is unchanged). The native file writes and the daemon's async Background-portal call (`fodderd/portal.rs`) are platform glue, not unit-tested.
- **`core/tests/conditional_get.rs`** - the conditional-GET path against a `wiremock` server: conditional headers actually sent (verified via the recorded request), 304 handling, validator capture, 429 with seconds and HTTP-date, and non-2xx -> error.
- **`fodder/src/reader.rs`** - HTML->Pango: scripts stripped, basic formatting converted, entities escaped, safe links kept.
- **`fodderd/src/reminder.rs`** - the next-occurrence time math stays within a day.
- **`fodderd/src/self_update.rs`** - the binary-signature comparison detects an in-place replacement (and a missing path reads as `None`); the watch loop and `exec` handoff are platform glue, not unit-tested.
- **`fodderd/src/tray.rs`** - the pure tray helpers: `rgba_to_argb`/`rgb_to_argb` channel reordering and that the embedded PNGs decode to correctly-sized pixmaps. The `ksni` wiring and the `wait_for_session_end` D-Bus watch are platform glue, not unit-tested (exercised manually via the running daemon + `gdbus`).

The GTK4 widget code in `fodder/src/app.rs`, the tokio<->glib bridge, and the daemon's async task wiring are **not** unit-tested; the daemon's IPC/lifecycle behavior is exercised manually via the `ctl` example and isolated shell runs.

### Android

All tests are JVM tests under `android/app/src/test/` - no emulator, so CI needs no device. They intentionally mirror the Rust suites named above, so a rule that changes on one platform has a failing test on the other if it is missed.

- **`FeedParserTest`** - RSS title + items, `<content:encoded>` preferred over `<description>`, Atom entries with `rel="alternate"` link preference, JSON Feed, the hashed-guid fallback being stable across parses, non-feed HTML/JSON parsing to null, and the RFC 3339 / RFC 822 date forms agreeing on the same instant.
- **`DedupeTest`** - id-when-present, SHA-256 link+title fallback (deterministic, idempotent, field boundaries kept distinct by the separator).
- **`ConditionalGetTest`** - against `MockWebServer`, the Android counterpart of `core/tests/conditional_get.rs`: conditional headers actually sent, 304, validator capture, 429 with seconds, 503 without a hint, non-2xx to error, plus `parseRetryAfter` for seconds / HTTP-date / past-date-clamps-to-zero / garbage.
- **`BackoffAndSummaryTest`** - backoff growth and the six-hour cap (including `Int.MAX_VALUE` and negative counts), and every branch of `formatRefreshSummary`.
- **`DiscoveryTest`** - multi-candidate extraction with relative-href resolution, JSON-feed recognition, non-feed alternates ignored, MIME-with-charset and rel-list matching.
- **`HtmlToAnnotatedTest`** - scripts/styles/iframes stripped, formatting and headings converted, entities decoded, safe links kept and `javascript:` hrefs dropped.
- **`ArticleDaoTest`** (Robolectric, in-memory Room) - the `(feedId, guid)` dedupe returning no new ids on re-poll, the same guid in two feeds staying two articles, cascade delete, unread counts across mark-read/unread and mark-all, success clearing the error while storing validators, and `reschedule` preserving validators. It runs on a plain `Application`, not `FodderApp`, so booting it does not schedule WorkManager.

ViewModels, Compose UI, `PollWorker`, `Notifier`, and `AppContainer` are **not** unit-tested - they need the Android framework or the Compose test harness. The established pattern is the same as on the desktop: pull the decidable logic out into a plain function or a constructor-injected class (`FeedRepository` takes its clock and poll spacing as parameters for exactly this reason) and test that.
