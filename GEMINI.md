# Fodder Reader Project (GEMINI.md)

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
4. **Wait 5 minutes for automated review comments** (e.g. Gemini Code Assist), then fetch
   them (`gh api repos/<owner>/<repo>/pulls/<n>/comments`). Validate each comment against
   the actual code - reviewers can be stale or wrong. Address the valid ones with commits
   on the same branch; reply to invalid/stale ones explaining why. Resolve the review
   threads you have handled (GraphQL `resolveReviewThread`), don't just reply.
5. Wait for CI to pass.
6. **Never merge a PR - merging is always the user's decision and action**, even when CI
   is green and all review comments are addressed. Stop when the PR is ready and report
   its URL.
7. After the user merges, releases are cut by tagging `main` with `vX.Y.Z` (never from a
   feature branch); the `release.yml` workflow builds the packages and updates the
   self-hosted apt/flatpak repos. See `docs/RELEASING.md`.

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

Before opening a PR, re-read these files and reconcile anything the change made inaccurate. Treat doc updates as part of "done," not a follow-up.

---

## Keep tests meaningful - IMPORTANT

For every change, add or update tests when doing so is meaningful - treat it as part of "done," not a follow-up. "Meaningful" means the test would actually catch a regression in the behavior you changed:

- New or changed logic with a testable contract (parsing, decisions, data transforms, DB queries, HTTP request/response handling) -> add or update unit tests covering the new behavior and its edge cases.
- Fixing a bug -> add a test that fails without the fix, so it can't silently regress.
- When the meaningful logic is tangled with hard-to-test platform code (GTK4 widgets, the tokio<->glib bridge), **extract the pure logic into a standalone function in `core` and test that** - this is the established pattern.
- Run the suite before opening a PR: `cargo test --workspace` (plus `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`, which CI enforces).

Skip new tests only when a change genuinely has no testable behavior (docs, comments, pure formatting, workflow YAML, trivial constant tweaks) - and say so briefly rather than silently omitting them.

---

## Project Overview

**Fodder** is a lightweight RSS/Atom/JSON-Feed reader for Linux desktops (GNOME, KDE, XFCE/Sway). It is built around a small, always-resident **daemon** that polls feeds and sends notifications, and a **viewer** spawned only on demand and freed when closed - so idle memory stays low while updates stay live.

### Technologies

- **Language:** Rust (edition 2021), a Cargo workspace of three crates: `core` (library), `fodderd` (daemon binary), `fodder` (viewer binary).
- **Daemon (`fodderd`):** `tokio` async runtime (no GTK). `reqwest` (rustls) for HTTP with conditional GET; `feed-rs` for parsing; `rusqlite` (bundled SQLite, WAL) for storage; `ksni` for the StatusNotifierItem tray; `notify-rust` for desktop notifications. Communicates with the viewer over a length-prefixed-JSON Unix socket in `$XDG_RUNTIME_DIR`.
- **Viewer (`fodder`):** GTK4 + libadwaita (`gtk4`/`libadwaita` 0.11/0.9 bindings). A 3-pane `OverlaySplitView`/`NavigationSplitView` UI. The reader defaults to a sanitized light renderer (`ammonia` -> Pango) with a toggle to a full `webkit6` (WebKitGTK 6.0) web view. Blocking DB work runs on the tokio pool and is applied on the GTK main thread via `glib::spawn_future_local`.
- **Feed parsing/discovery:** `scraper` + `ego-tree` for `<link rel="alternate">` discovery and the HTML->Pango reader.
- **App ID / icon:** `io.github.dipakmdhrm.Fodder`; a hicolor icon set + a `.desktop` entry (installed by the packaging / `install.sh`).

### Architecture

> **Authoritative architecture documentation lives in `CLAUDE.md`** (per-crate/per-module
> detail, the process model, and test-coverage boundaries) and is kept in sync with the
> code on every PR. The summary below is intentionally brief - when it disagrees with
> CLAUDE.md, CLAUDE.md is right.

- `core/` (`fodder-core`): models, TOML config, SQLite store + `user_version` migrations, the HTTP poller (conditional GET, backoff, GUID dedupe via `INSERT OR IGNORE`), feed discovery, the IPC protocol, autostart, and XDG paths. Holds essentially all the pure, unit-tested logic.
- `fodderd/`: the headless daemon - single-instance socket guard, IPC server, poll scheduler, batched actionable notifications + a daily reading reminder, the `ksni` tray (graceful-degrade where no SNI host exists), and on-demand viewer spawn/reap. Config is live-reloaded on a `ReloadConfig` message. The daemon also remembers the viewer's last-read article + mode (in memory) and restores it on the next open.
- `fodder/`: the GTK4 viewer - 3-pane UI, the tokio<->glib bridge, the IPC client, and the light/WebKit reader. The WebKit view uses a dedicated `WebContext` and `terminate_web_process()` so its subprocesses' memory is reclaimed on toggle-back.

**Process model:** `fodderd` is primary and resident; `fodder` is spawned on demand and freed on close. Exactly one daemon and one viewer, enforced via the runtime socket (which also carries IPC). Storage: `~/.config/fodder/config.toml` and `~/.local/share/fodder/db.sqlite` (WAL). On a package upgrade (release builds), the resident daemon detects its replaced binary (`fodderd/src/self_update.rs`) and re-execs the new one in place, so the tray survives `apt upgrade`; the deb `prerm` therefore only stops the daemon on removal, not upgrade.

## Building and Running

```bash
# Build / test the whole workspace
cargo build --workspace
cargo test --workspace

# Lint + format (CI enforces both)
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Run the daemon (foreground, with logs)
RUST_LOG=info cargo run -p fodderd

# Run the viewer directly (connects back to the daemon)
cargo run -p fodder
```

**Dev gotcha:** `cargo run -p fodderd` does not rebuild the viewer - the daemon spawns the
`target/debug/fodder` binary by path, so run `cargo build --workspace` first after changing
viewer code, or a stale viewer will be launched.

Per-user install without packaging: `./install.sh` / `./uninstall.sh [--purge]`.

## Development Conventions

### Continuous Integration

`.github/workflows/ci.yml` runs on every pull request to `main`: first `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (installing the GTK4/libadwaita/WebKitGTK dev libraries first); then it builds all four packages via the shared reusable workflow so a release tag is just a repeat of an already-green build. The tree is kept fmt-clean and clippy-clean.

### Release Process

The four package builds (`.deb`, `.rpm`, an Arch `.pkg.tar.zst`, and Flatpak bundles; x86_64 + arm64, Arch x86_64-only) live in one reusable workflow, `.github/workflows/build-packages.yml`, called by both `ci.yml` and `release.yml`. `release.yml` is triggered by a `v*` tag: it runs that shared build, attaches all packages to a GitHub Release, and publishes GPG-signed **apt** and **flatpak** repositories to the `gh-pages` branch so `.deb` and Flatpak installs auto-update. The Arch build links the **system** SQLite (its PKGBUILD drops rusqlite's `bundled` feature) because makepkg's hardening link flags break the vendored static SQLite. Packaging sources are under `packaging/`; the full process and one-time setup (GPG key secret, GitHub Pages) are documented in `docs/RELEASING.md`.
