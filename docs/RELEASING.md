# Releasing Fodder

The repo ships two products from one branch, each on its own tag namespace:

| Product | Source | Tag | Workflow | Artifacts |
|---------|--------|-----|----------|-----------|
| Linux | `linux/` | `vX.Y.Z` | `release.yml` | `.deb`, `.rpm`, Arch, Flatpak + the self-hosted apt/flatpak repos |
| Android | `android/` | `android-X.Y.Z` | `release-android.yml` | a signed APK on the GitHub Release |

A Linux release builds `.deb`, `.rpm`, an Arch package, and Flatpak bundles
(x86_64 + arm64, except Arch which is x86_64-only), attaches them to a GitHub
Release, and updates the self-hosted **apt** and **flatpak** repositories on the
`gh-pages` branch so existing installs auto-update.

Releases are cut **automatically on every merge to `main`** — see
[Automatic releases](#automatic-releases) — for whichever platform the merge
touched. Pushing a `vX.Y.Z` or `android-X.Y.Z` tag by hand still works too, for
manual/backfill releases. Android signing setup lives in [ANDROID.md](ANDROID.md).

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

5. **Flathub auto-publish** *(only after the app is accepted on Flathub)*. Once
   Flathub has created the `flathub/io.github.dipakmdhrm.Fodder` repo, wire up
   automatic update PRs (see [Flathub auto-publish](#flathub-auto-publish)):
   - Create a **fine-grained PAT** with *Contents: write* + *Pull requests:
     write*, scoped to just that one Flathub repo, and add it as the repo secret
     **`FLATHUB_TOKEN`**.
   - Set the repo **variable** `FLATHUB_AUTOPUBLISH` = `true` (Settings → Secrets
     and variables → Actions → *Variables*). This is the on-switch; until it's
     `true`, the publisher never runs.
   - In the Flathub repo's settings, enable **Allow auto-merge** so the opened PR
     lands itself once Flathub's Buildbot check is green.

## Automatic releases

Merging a PR to `main` cuts a release with no further action — merging the
feature PR is the only gate. The `auto-release.yml` workflow:

1. Reads the merged PR's `release:*` label to choose the bump:
   - `release:major` → `X+1.0.0`
   - `release:minor` → `X.Y+1.0`
   - *(no label)* → `X.Y.Z+1` (patch, the default)
   - `release:skip` → no release for this merge
2. **Releases only the platform the merge touched**, by looking at whether the
   merged range changed anything under `linux/` or `android/`. A merge that
   changes only `.github/`, `docs/`, `*.md`, or root files ships nothing on
   either platform, so it doesn't bump anything. An explicit
   `release:major`/`release:minor` label on such a merge forces a **Linux**
   release anyway (the historical escape hatch).
3. **Linux**: computes the next version from the newest `v*` tag, bumps
   `[workspace.package] version` in `linux/Cargo.toml`, syncs `linux/Cargo.lock`
   (`cargo update --workspace`, workspace members only), and stamps
   `CHANGELOG.md` (moves `## Unreleased` to the new version, leaving a fresh
   empty `## Unreleased`). Commits `Release vX.Y.Z [skip ci]` to `main`, pushes
   an annotated `vX.Y.Z` tag, and invokes `release.yml` (via `workflow_call`,
   pointed at the new tag) to build and publish exactly as a manual tag push
   would.
4. **Android**: computes the next version from the newest `android-*` tag and
   pushes an `android-X.Y.Z` tag, then invokes `release-android.yml`. There is
   nothing to commit — the APK's `versionName`/`versionCode` come from the tag
   at build time, so the tag is the only version record.

**No release loop.** The bump commit and the tag are pushed with the default
`GITHUB_TOKEN`, and pushes made with `GITHUB_TOKEN` do not trigger further
workflow runs. That is also why `auto-release.yml` calls `release.yml` through
`workflow_call` instead of relying on its `push: tags` trigger — the
token-pushed tag would not fire it.

So `linux/Cargo.toml` and `linux/Cargo.lock` on `main` always reflect the latest
Linux release, and a local `cargo build` reports the right version. Curate `CHANGELOG.md` under
`## Unreleased` as part of normal PR work; the release stamps it for you.

## Flathub auto-publish

Once configured (setup step 5), every release also proposes a matching update to
the app's Flathub repo — no manual manifest bump. The `flathub-publish.yml`
reusable workflow, invoked by `release.yml` (so it fires for **both**
auto-releases and manual tag pushes), does:

1. Checks out the app repo at the release tag and resolves its commit sha.
2. Regenerates `cargo-sources.json` from the tagged `Cargo.lock` (crates may have
   changed since the last release).
3. Clones `flathub/io.github.dipakmdhrm.Fodder`, repoints the manifest's git
   source at the new `tag` + `commit` (targeted `sed`, so the PR diff is just
   those two lines plus the vendored sources), and copies the regenerated
   `cargo-sources.json` in.
4. Pushes an `update-vX.Y.Z` branch and opens a PR to the Flathub repo, then
   enables auto-merge so Flathub's **Buildbot** test-builds it and lands it on
   green.

It is **dormant by default**: `release.yml` only calls it when the repo variable
`FLATHUB_AUTOPUBLISH == 'true'`, and the workflow additionally no-ops if the
`FLATHUB_TOKEN` secret is missing. You can trigger it by hand for a re-run via
the Actions tab (**Flathub Publish** → *Run workflow* → enter the version).

> Note: this path can't be exercised until the Flathub repo exists, so treat the
> first real run as something to watch. If auto-merge isn't enabled on the
> Flathub repo, the workflow still opens the PR — just merge it manually once
> Buildbot is green.

## Cutting a release manually

You rarely need this (merges auto-release), but a hand-pushed tag still works —
e.g. to re-cut a build or release off-cycle:

**Linux:**

1. Bump the version in `linux/Cargo.toml` (`[workspace.package] version`) and
   update `CHANGELOG.md`.
2. Commit, then tag and push:
   ```bash
   git tag v0.1.0
   git push origin main --tags
   ```
3. The **Release** workflow runs (its `push: tags` trigger). When it finishes
   you'll have a GitHub Release with all packages, and the apt/flatpak repos on
   `gh-pages` will be updated.

**Android:** nothing to bump — just push the tag.

```bash
git tag android-0.2.0
git push origin android-0.2.0
```

## Notes / expectations

- **CI** (`ci.yml`) runs `cargo fmt --check`, `cargo clippy -D warnings`, and
  `cargo test` in `linux/`, plus `ktlintCheck`, the JVM unit tests, and
  `assembleDebug` in `android/`, on every PR. Both jobs run unconditionally, so
  a required status check is never left "expected" on a single-platform PR.
- **Arch** is x86_64-only (Arch Linux's official architecture). `.deb`, `.rpm`,
  and Flatpak are dual-arch.
- The self-hosted **Flatpak** build (`linux/packaging/flatpak/`) fetches crates
  over the network. The **Flathub** build
  (`linux/packaging/flatpak/flathub/`) instead builds fully offline from a
  vendored `cargo-sources.json` and builds with `subdir: linux`; releases keep it
  current automatically via [Flathub auto-publish](#flathub-auto-publish).
- The multi-arch CI (reprepro apt repo, ostree flatpak repo, GPG in Actions,
  arm64 runners) can't be validated locally — expect to iterate on the first
  real run. The GitHub Release job is independent of the repo-publishing job, so
  package artifacts still upload even if repo publishing needs a tweak.
