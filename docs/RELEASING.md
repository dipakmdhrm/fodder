# Releasing Fodder

Releases are built by GitHub Actions. Pushing a `vX.Y.Z` tag builds `.deb`,
`.rpm`, an Arch package, and Flatpak bundles (x86_64 + arm64, except Arch which
is x86_64-only), attaches them to a GitHub Release, and updates the self-hosted
**apt** and **flatpak** repositories on the `gh-pages` branch so existing
installs auto-update.

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

## Cutting a release

1. Bump the version in the workspace `Cargo.toml` (`[workspace.package] version`)
   and update `CHANGELOG.md`.
2. Commit, then tag and push:
   ```bash
   git tag v0.1.0
   git push origin main --tags
   ```
3. The **Release** workflow runs. When it finishes you'll have a GitHub Release
   with all packages, and the apt/flatpak repos on `gh-pages` will be updated.

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
