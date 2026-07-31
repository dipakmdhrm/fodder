# Releasing Fodder

Releases are built by GitHub Actions. A release builds `.deb`, `.rpm`, an Arch
package, and Flatpak bundles (x86_64 + arm64, except Arch which is x86_64-only),
attaches them to a GitHub Release, and updates the self-hosted **apt** and
**flatpak** repositories on the `gh-pages` branch so existing installs
auto-update.

Releases are cut **automatically on every merge to `main`** — see
[Automatic releases](#automatic-releases). Pushing a `vX.Y.Z` tag by hand still
works too, for manual/backfill releases.

## One-time setup

1. **Create the GitHub repo** and push (`git remote add origin …`, `git push`).
   The workflows only run on GitHub.

2. **GPG signing key** (signs both the apt and flatpak repos). Generate once:
   ```bash
   gpg --batch --gen-key <<EOF
   Key-Type: RSA
   Key-Length: 4096
   Name-Real: Fodder
   Name-Email: noreply@github.com
   Expire-Date: 0
   %no-passphrase
   EOF
   gpg --armor --export-secret-keys "Fodder" > private.key
   ```
   Add it as a repo secret **`APT_SIGNING_KEY`** (Settings → Secrets and
   variables → Actions), then delete `private.key`.

3. **Enable GitHub Pages**: Settings → Pages → Source = *Deploy from a branch* →
   Branch = `gh-pages`. (The branch is created by the first release.)

4. **Create the release labels** (used to pick the bump size on a merge):
   ```bash
   gh label create release:major --color B60205 --description "Auto-release: major bump"
   gh label create release:minor --color FBCA04 --description "Auto-release: minor bump"
   gh label create release:skip  --color 0E8A16 --description "Auto-release: skip this merge"
   ```
   Missing labels are tolerated (an unlabeled merge is a patch release); the
   labels just let you request minor/major or opt out.

## Automatic releases

Merging a PR to `main` cuts a release with no further action — merging the
feature PR is the only gate. The `auto-release.yml` workflow:

1. Reads the merged PR's `release:*` label to choose the bump:
   - `release:major` → `X+1.0.0`
   - `release:minor` → `X.Y+1.0`
   - *(no label)* → `X.Y.Z+1` (patch, the default)
   - `release:skip` → no release for this merge
2. Computes the next version from the newest `v*` tag.
3. Bumps `[workspace.package] version` in `Cargo.toml`, syncs `Cargo.lock`
   (`cargo update --workspace`, workspace members only), and stamps
   `CHANGELOG.md` (moves `## Unreleased` to the new version, leaving a fresh
   empty `## Unreleased`).
4. Commits `Release vX.Y.Z [skip ci]` to `main` and pushes an annotated
   `vX.Y.Z` tag.
5. Invokes `release.yml` (via `workflow_call`, pointed at the new tag) to build
   and publish exactly as a manual tag push would.

**No release loop.** The bump commit and the tag are pushed with the default
`GITHUB_TOKEN`, and pushes made with `GITHUB_TOKEN` do not trigger further
workflow runs. That is also why `auto-release.yml` calls `release.yml` through
`workflow_call` instead of relying on its `push: tags` trigger — the
token-pushed tag would not fire it.

So `Cargo.toml`/`Cargo.lock` on `main` always reflect the latest release, and a
local `cargo build` reports the right version. Curate `CHANGELOG.md` under
`## Unreleased` as part of normal PR work; the release stamps it for you.

## Cutting a release manually

You rarely need this (merges auto-release), but a hand-pushed tag still works —
e.g. to re-cut a build or release off-cycle:

1. Bump the version in the workspace `Cargo.toml` (`[workspace.package] version`)
   and update `CHANGELOG.md`.
2. Commit, then tag and push:
   ```bash
   git tag v0.1.0
   git push origin main --tags
   ```
3. The **Release** workflow runs (its `push: tags` trigger). When it finishes
   you'll have a GitHub Release with all packages, and the apt/flatpak repos on
   `gh-pages` will be updated.

## Notes / expectations

- **CI** (`ci.yml`) runs `cargo fmt --check`, `cargo clippy -D warnings`, and
  `cargo test` on every PR.
- **Arch** is x86_64-only (Arch Linux's official architecture). `.deb`, `.rpm`,
  and Flatpak are dual-arch.
- The **Flatpak** build fetches crates over the network (fine for a self-hosted
  repo). A future Flathub submission would instead vendor crates via a generated
  `cargo-sources.json`.
- The multi-arch CI (reprepro apt repo, ostree flatpak repo, GPG in Actions,
  arm64 runners) can't be validated locally — expect to iterate on the first
  real run. The GitHub Release job is independent of the repo-publishing job, so
  package artifacts still upload even if repo publishing needs a tweak.
