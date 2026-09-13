# Fodder Reader Project (GEMINI.md)

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
   platform the merge touched: `auto-release.yml` (on push to `main`) reads the
   merged PR's `release:*` label for the bump size (default patch;
   `release:skip` opts out), then a merge under `linux/` bumps
   `linux/Cargo.toml`/`linux/Cargo.lock`, stamps `CHANGELOG.md`, tags `vX.Y.Z`
   and hands off to `release.yml`, while a merge under `android/` tags
   `android-X.Y.Z` and hands off to `release-android.yml` (no file to bump - the
   APK takes its version from the tag). A merge touching only `.github/`,
   `docs/`, `*.md`, or root files is skipped (a `release:*` bump label forces a
   Linux release). Label
   the PR `release:minor`/`release:major` when appropriate, or `release:skip` to
   merge without releasing. A manual `vX.Y.Z` tag push still works for off-cycle
   releases. See `docs/RELEASING.md`.

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

Before opening a PR, re-read these files and reconcile anything the change made inaccurate. Treat doc updates as part of "done," not a follow-up.

---

## Keep tests meaningful - IMPORTANT

For every change, add or update tests when doing so is meaningful - treat it as part of "done," not a follow-up. "Meaningful" means the test would actually catch a regression in the behavior you changed:

- New or changed logic with a testable contract (parsing, decisions, data transforms, DB queries, HTTP request/response handling) -> add or update unit tests covering the new behavior and its edge cases.
- Fixing a bug -> add a test that fails without the fix, so it can't silently regress.
- When the meaningful logic is tangled with hard-to-test platform code (GTK4 widgets, the tokio<->glib bridge, Android ViewModels/Compose), **extract the pure logic into a standalone function and test that** - this is the established pattern on both platforms.
- Run the suite before opening a PR: in `linux/`, `cargo test --workspace` (plus `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`); in `android/`, `./gradlew ktlintCheck testDebugUnitTest`. CI enforces all of them.
- **Porting rule:** a rule both platforms implement (feed parsing, dedupe, conditional-GET classification, backoff, the refresh summary, HTML sanitizing) must change on both sides in the same PR, with both tests updated. Each Kotlin port names its Rust counterpart in a KDoc comment.

Skip new tests only when a change genuinely has no testable behavior (docs, comments, pure formatting, workflow YAML, trivial constant tweaks) - and say so briefly rather than silently omitting them.

---

## Project Overview

**Fodder** is a lightweight RSS/Atom/JSON-Feed reader shipping two independent apps from one repo: a Linux desktop app in `linux/` (Rust) and an Android app in `android/` (Kotlin + Jetpack Compose). They share a design and a set of behavioral rules, not code - separate subscriptions, separate databases, no sync - and release on separate tag namespaces (`vX.Y.Z` and `android-X.Y.Z`).

The Linux app targets GNOME, KDE, and XFCE/Sway. It is built around a small, always-resident **daemon** that polls feeds and sends notifications, and a **viewer** spawned on demand and, by default, kept resident between opens for an instant reopen while the daemon polls in the background. A `low_memory_mode` setting (default off) instead frees the viewer when its window is closed, reclaiming its memory at the cost of a cold reopen.

### Technologies

- **Language (Linux):** Rust (edition 2021), a Cargo workspace under `linux/` of three crates: `core` (library), `fodderd` (daemon binary), `fodder` (viewer binary).
- **Language (Android):** Kotlin, a single-module Gradle project under `android/`: Jetpack Compose + Material 3, Room, OkHttp, WorkManager, jsoup, `minSdk` 26. No daemon and no IPC - one process owns the database, the poller, and the UI. Versions come from the git tag at build time.
- **Daemon (`fodderd`):** `tokio` async runtime (no GTK). `reqwest` (rustls) for HTTP with conditional GET; `feed-rs` for parsing; `rusqlite` (bundled SQLite, WAL) for storage; `ksni` for the StatusNotifierItem tray; `notify-rust` for desktop notifications. Communicates with the viewer over a length-prefixed-JSON Unix socket in `$XDG_RUNTIME_DIR`.
- **Viewer (`fodder`):** GTK4 + libadwaita (`gtk4`/`libadwaita` 0.11/0.9 bindings). A 3-pane `OverlaySplitView`/`NavigationSplitView` UI. The reader defaults to a sanitized light renderer (`ammonia` -> Pango) with a toggle to a full `webkit6` (WebKitGTK 6.0) web view. Blocking DB work runs on the tokio pool and is applied on the GTK main thread via `glib::spawn_future_local`.
- **Feed parsing/discovery:** `scraper` + `ego-tree` for `<link rel="alternate">` discovery and the HTML->Pango reader.
- **App ID / icon:** `io.github.dipakmdhrm.Fodder`; a hicolor icon set + a `.desktop` entry (installed by the packaging / `install.sh`).

### Architecture

> **Authoritative architecture documentation lives in `CLAUDE.md`** (per-crate/per-module
> detail, the process model, and test-coverage boundaries) and is kept in sync with the
> code on every PR. The summary below is intentionally brief - when it disagrees with
> CLAUDE.md, CLAUDE.md is right.

- `linux/core/` (`fodder-core`): models, TOML config, SQLite store + `user_version` migrations, the HTTP poller (conditional GET, backoff, GUID dedupe via `INSERT OR IGNORE`, guarded by the `deleted_articles` tombstones so a user-deleted item never returns), feed discovery, the IPC protocol, autostart, and XDG paths. Holds essentially all the pure, unit-tested logic.
- `linux/fodderd/`: the headless daemon - single-instance socket guard, IPC server, poll scheduler, batched actionable notifications + a daily reading reminder, the `ksni` tray (graceful-degrade where no SNI host exists), kept simple by tying it to the session: registered with `disable_dbus_name(true)` (unique connection name — works in Flatpak, no restart collisions) + `assume_sni_available(true)` (tolerates autostarting before the host is up; ksni re-registers across watcher restarts), the app icon embedded as ARGB pixmaps so hosts that mishandle a themed `IconName` still render it, and the daemon exits when the D-Bus session connection dies (logout) so autostart brings up a fresh one; plus on-demand viewer spawn/reap. Config is live-reloaded on a `ReloadConfig` message. The daemon also remembers the viewer's last-read article + mode (in memory) and restores it on the next open.
- `linux/fodder/`: the GTK4 viewer - 3-pane UI, the tokio<->glib bridge, the IPC client, and the light/WebKit reader. The context menus can delete a single article or all of one feed's articles (keeping the subscription); both, like feed deletion, go through a `confirm_destructive` dialog and write straight to SQLite rather than through the daemon. The WebKit view uses a dedicated `WebContext` and `terminate_web_process()` so its subprocesses' memory is reclaimed on toggle-back. The header-bar gear is a `MenuButton` (Preferences / About Fodder); About renders an `adw::AboutDialog`. A user-invoked refresh (button/context-menu/tray) spins the refresh button and, on completion, raises an `adw::Toast` summarizing the outcome, driven by the daemon's `RefreshStarted`/`RefreshFinished` IPC messages and `fodder_core::refresh::format_refresh_summary`.

Both binaries accept `--version`/`-V`, which prints the shared `fodder_core::version_blurb(...)` (name, version, description, homepage, license) and exits without launching.

- `android/app/`: the Compose app - `FodderApp`/`AppContainer` (manual DI), Room (`data/db/`), the poll-and-store workflow in `data/FeedRepository.kt` (the counterpart of the daemon's scheduler), a WorkManager `PollWorker` with batched per-feed notifications, and the ported pure logic (`data/feed/`, `data/http/`, `reader/`, `work/RefreshSummary.kt`), each file naming its Rust counterpart. It carries the same delete actions and the same tombstone rule as the desktop (`DeletedArticleEntity`, Room `MIGRATION_1_2`). Android's periodic-work floor is 15 minutes and Doze can delay a run, so background refresh is best-effort and pull-to-refresh is the way to force a poll.

**Process model:** `fodderd` is the primary daemon, resident for the graphical session (it exits on logout when the session bus goes away; autostart relaunches it at the next login); `fodder` is spawned on demand and, by default, kept resident on close - closing hides the window (keeping the live WebView too) for an instant reopen; `low_memory_mode` (default off) instead frees the process on close. The choice lives viewer-side in `App::low_memory`, no daemon change. Exactly one daemon and one viewer, enforced via the runtime socket (which also carries IPC). Storage: `~/.config/fodder/config.toml` and `~/.local/share/fodder/db.sqlite` (WAL). On a package upgrade (release builds), the resident daemon detects its replaced binary (`fodderd/src/self_update.rs`) and re-execs the new one in place, so the tray survives `apt upgrade`; the deb `prerm` therefore only stops the daemon on removal, not upgrade.

## Building and Running

Rust commands run from `linux/`, Gradle commands from `android/`.

```bash
# --- linux/ ---
cargo build --workspace
cargo test --workspace

# Lint + format (CI enforces both)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Run the daemon (foreground, with logs)
RUST_LOG=info cargo run -p fodderd

# Run the viewer directly (connects back to the daemon)
cargo run -p fodder

# --- android/ ---
./gradlew testDebugUnitTest    # JVM unit tests; no emulator needed
./gradlew ktlintCheck          # lint; ktlintFormat fixes most findings
./gradlew assembleDebug
```

**Dev gotcha:** `cargo run -p fodderd` does not rebuild the viewer - the daemon spawns the
`target/debug/fodder` binary by path, so run `cargo build --workspace` first after changing
viewer code, or a stale viewer will be launched.

Per-user install without packaging: `linux/install.sh` / `linux/uninstall.sh [--purge]`.

## Build artifact hygiene

`linux/target/` grows without bound if nobody trims it. Cargo never garbage-collects that
directory, so every dependency bump, feature change, or toolchain update emits a fresh
hash-suffixed artifact and keeps the previous one forever. Five weeks of unmaintained
development took this repo to 36 GB, almost all of it stale copies of binaries.

Size any cleanup decision against two measured facts:

- Building the workspace alone lands at about **1.4 GB**; a full check cycle (`build`, then `clippy --all-targets`, then `test --workspace`) settles at about **2.2 GB**. The `[profile.dev.package."*"]` override in `linux/Cargo.toml` is what keeps it there. Without it the build alone is 2.9 GB, because every dependency then carries full DWARF.
- A full cold rebuild takes about **60 seconds**, so cleaning is cheap. Do not treat it as a last resort.

After a session that ran cargo builds, check the size and report it when it exceeds
**6 GB**. That is roughly 3x the full-cycle steady state, and means stale artifacts have
piled up:

```bash
du -sh linux/target
```

Trim in this order:

```bash
rm -rf linux/target/debug/incremental   # regenerates on the next build; always safe
cargo sweep --maxsize 4GB               # run from linux/; keeps newest, drops oldest
cargo clean                             # full reset; about 60s to rebuild
```

Three things to get right:

- Run `cargo-sweep` from `linux/`, never the repo root. There is no root `Cargo.toml`, so `cargo metadata` fails there with "manifest path does not exist".
- Do not use `cargo sweep --installed` as routine cleanup. It keeps only artifacts built by the currently installed rustc, so right after a `rustup update` it discards every artifact rather than just the stale ones.
- Always report how much was reclaimed. Never delete build output silently.

---

## Development Conventions

### Continuous Integration

`.github/workflows/ci.yml` runs on every pull request to `main`. The Rust job (working directory `linux/`) runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (installing the GTK4/libadwaita/WebKitGTK dev libraries first), then builds all four packages via the shared reusable workflow so a release tag is just a repeat of an already-green build. A parallel Android job runs `ktlintCheck`, the JVM unit tests, a Jacoco report, and `assembleDebug`. Both jobs are unconditional (no path filters) so a required status check is never left "expected" on a single-platform PR. The tree is kept fmt-clean, clippy-clean, and ktlint-clean.

### Release Process

The four package builds (`.deb`, `.rpm`, an Arch `.pkg.tar.zst`, and Flatpak bundles; x86_64 + arm64, Arch x86_64-only) live in one reusable workflow, `.github/workflows/build-packages.yml` (parameterized by `version` and an optional `ref` to check out), called by both `ci.yml` and `release.yml`. `release.yml` runs on a `v*` tag **or** `workflow_call`: it runs that shared build, attaches all packages to a GitHub Release, and publishes GPG-signed **apt** and **flatpak** repositories to the `gh-pages` branch so `.deb` and Flatpak installs auto-update. Releases are cut **automatically on merge to `main`** by `.github/workflows/auto-release.yml`, per platform: a merge touching `linux/` derives the next version from the newest `v*` tag and the merged PR's `release:*` label, bumps `linux/Cargo.toml`/`linux/Cargo.lock`, stamps `CHANGELOG.md`, commits + tags, and invokes `release.yml` (via `workflow_call`, pointed at the new tag); a merge touching `android/` instead pushes an `android-X.Y.Z` tag and invokes `.github/workflows/release-android.yml`, which decodes the signing keystore from repository secrets, builds a signed APK, and attaches it to a GitHub Release (setup in `docs/ANDROID.md`). There is no release loop because it pushes with `GITHUB_TOKEN` (whose pushes don't retrigger workflows) and reaches `release.yml` through `workflow_call` rather than the token-pushed tag. The Arch build links the **system** SQLite (its PKGBUILD drops rusqlite's `bundled` feature) because makepkg's hardening link flags break the vendored static SQLite. Packaging sources are under `linux/packaging/`; the full process and one-time setup (GPG key secret, GitHub Pages, release labels) are documented in `docs/RELEASING.md`. A **Flathub** submission is prepared under `linux/packaging/flatpak/flathub/`: unlike the self-hosted `linux/packaging/flatpak/` manifest (curls rustup, builds online), it builds **fully offline** (Rust from the `rust-stable` SDK extension; crates vendored via a generated `cargo-sources.json` with `CARGO_NET_OFFLINE=true`), pins sources to a git tag + commit, and ships the AppStream metainfo `linux/data/metainfo/io.github.dipakmdhrm.Fodder.metainfo.xml`. Both Flatpak manifests build from the `linux/` workspace (the self-hosted one via its `path: ../..` source, the Flathub one via `subdir: linux`). Regenerate `cargo-sources.json` (`flatpak-cargo-generator.py linux/Cargo.lock`) when deps change, and lint with `flatpak-builder-lint` (portals take **no** `--talk-name`; `fallback-x11` needs `--share=ipc`). Autostart-on-login uses the XDG **Background** portal inside Flatpak (`fodderd/portal.rs`, driven from the viewer via the `SetAutostart` IPC message) since writing `~/.config/autostart` is a no-op in the sandbox; native installs still write the autostart `.desktop`. Once the app is live on Flathub, `flathub-publish.yml` (invoked by `release.yml`) auto-proposes each release to `flathub/io.github.dipakmdhrm.Fodder`: it regenerates `cargo-sources.json`, repoints the manifest at the new tag+commit, and opens an auto-merging update PR — dormant until `FLATHUB_AUTOPUBLISH=true` + the `FLATHUB_TOKEN` secret are set.
