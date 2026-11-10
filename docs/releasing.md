# Releasing kyko

A release happens by itself when the version on `main` changes. You bump the version, write the
changelog, and merge. The workflow does the rest.

## Versions

- The workspace `version` in `Cargo.toml` is the only version. `packaging/kyko/PKGBUILD` follows
  it. `tests/shell/version_test.sh` (run by CI) fails when they differ or when `.SRCINFO` is stale.
- Tags are `vX.Y.Z` or, for release candidates, `vX.Y.Z-rc.N`. The workflow creates the tag. You
  do not.
- pacman forbids `-` in `pkgver`. A release candidate's `pkgver` is `X.Y.ZrcN`
  (`1.0.0-rc.1` becomes `1.0.0rc1`). The PKGBUILD maps it back with `_tag=${pkgver/rc/-rc.}`.
- Anything below 1.0.0, and every release candidate, is published as a GitHub pre-release.

## Release gate

Before you merge a version bump to `main`:

1. CI is green on the branch.
2. `CHANGELOG.md` has a section for the new version, with text in it.
3. The live checklist for the milestone is done, with no open Critical or Important issues.

CI runs again inside the release workflow. If it fails, nothing is published.

## Cutting a release

1. Move the `[Unreleased]` notes in `CHANGELOG.md` under `## [X.Y.Z] - YYYY-MM-DD`. Leave an empty
   `[Unreleased]` section above it.
2. Run `scripts/bump-version.sh X.Y.Z` (or `X.Y.Z-rc.N`). It updates `Cargo.toml`, `Cargo.lock`,
   `pkgver`, resets `pkgrel=1`, and regenerates `packaging/kyko/.SRCINFO`.
3. Run `bash tests/shell/version_test.sh`. Commit (`release: vX.Y.Z`) and merge to `main`.
4. Watch the `Release` workflow. It does this, in order:
   - `decide` runs `scripts/release-needed.sh`. If the version already has a tag, the run ends
     green and nothing is released. That is normal for every ordinary push to `main`. It runs
     only in `asafelobotomy/kyko`, so a fork's `main` never releases.
   - `ci` runs the full CI.
   - `build`, with a read-only token, builds `kyko-X.Y.Z.tar.gz` with `git archive`, builds the
     package from it, installs it, and checks that `kyko --version` prints `kyko X.Y.Z` and
     `kykod --version` prints `kykod X.Y.Z`. It writes the release notes and the AUR files and
     hands everything to `publish` as workflow artifacts.
   - `publish` builds nothing and runs no project code. It checks the tarball against its
     `.sha256`, attests the tarball and the package, creates and pushes the tag `vX.Y.Z` on the
     commit `decide` judged, and publishes the GitHub Release. The release has the CHANGELOG
     section as notes, and three assets: the tarball, its `.sha256`, and
     `kyko-<pkgver>-<pkgrel>-x86_64.pkg.tar.zst`. The debug package is not published. The
     attestations are not assets: they live in GitHub's attestation store
     (`gh attestation verify kyko-X.Y.Z.tar.gz --repo asafelobotomy/kyko`).
   - `aur` pushes to the AUR, if the `AUR_SSH_KEY` secret exists (below).

## When the release is refused

`scripts/release-needed.sh` fails the run with one of these messages. Nothing is tagged or
published.

- `tag vX.Y.Z exists at another history`: a tag with this version exists, but it is not in the
  history of `main`. Someone made it by hand, or `main` was rewritten. Find out where the tag
  points (`git log -1 vX.Y.Z`). If the tag is wrong, delete it (`git push origin :refs/tags/vX.Y.Z`)
  and push again. If the version is wrong, bump to a new one.
- `X.Y.Z is lower than vA.B.C`: the version went backwards. Run `scripts/bump-version.sh` with a
  version above the newest tag, and merge that.
- `no CHANGELOG section for X.Y.Z`: `CHANGELOG.md` has no `## [X.Y.Z]` section, or it is empty.
  Write it and merge. No tag exists yet, so the next push to `main` tries again.
- A message that Cargo.toml has no valid version: fix `version` in the root `Cargo.toml`. It must
  look like `1.2.3` or `1.2.3-rc.1`.

If the run fails before the "Create and push the tag" step of `publish`, nothing is public. Fix
the cause and push to `main` again, or re-run the workflow.

If it fails after the tag was pushed (for example, creating the GitHub Release failed), the
release is half done, and it does not finish by itself: the version now has a tag, so a re-run
of the whole workflow, or any later push to `main`, gets `skip` from `decide` and ends green
with no release. To recover, delete any partial release (`gh release delete vX.Y.Z`), delete the
tag (`git push origin :refs/tags/vX.Y.Z`), then re-run the workflow (all jobs) or push to `main`
again.

## The AUR

The AUR package builds from the release asset `kyko-X.Y.Z.tar.gz`, not from GitHub's `/archive/`
tarball. Those are generated on demand and may differ byte for byte from `git archive`. So the
checksum in the PKGBUILD is the checksum of the published asset. The workflow computes it.

### Automatic push: add `AUR_SSH_KEY`

1. Make a key pair just for this: `ssh-keygen -t ed25519 -f aur-kyko -C "kyko release"`.
2. Add the public key (`aur-kyko.pub`) to your AUR account under "SSH Public Key".
3. In the GitHub repository, go to Settings, Secrets and variables, Actions. Add a secret named
   `AUR_SSH_KEY` with the contents of the private key file `aur-kyko`.
4. Delete the local key files, or keep them somewhere safe.

The `aur` job then clones `ssh://aur@aur.archlinux.org/kyko.git`, copies in the PKGBUILD and
`.SRCINFO`, commits `Update to X.Y.Z`, and pushes. It only trusts the AUR's published ed25519 host
key. The first push creates the `kyko` package on the AUR.

After an automatic push, `packaging/kyko/PKGBUILD` and `.SRCINFO` on `main` still carry the
previous checksum (the workflow does not commit to `main`) until you copy the two files from the
run's `aur` artifact into `packaging/kyko/` and commit them. That commit releases nothing: the
version already has a tag.

### Manual push: when the secret is absent

Without `AUR_SSH_KEY` (before you add it) the `aur` job is skipped and the release still
succeeds. The PKGBUILD and `.SRCINFO` are in the run summary and in the `aur` artifact.

1. Download the `aur` artifact from the workflow run.
2. Check the checksum against the release notes:

   ```bash
   curl -fLO https://github.com/asafelobotomy/kyko/releases/download/vX.Y.Z/kyko-X.Y.Z.tar.gz
   sha256sum kyko-X.Y.Z.tar.gz
   ```

3. Copy the two files into your AUR clone and push:

   ```bash
   git clone ssh://aur@aur.archlinux.org/kyko.git aur-kyko
   cp PKGBUILD .SRCINFO aur-kyko/
   cd aur-kyko && makepkg --verifysource && git add PKGBUILD .SRCINFO \
     && git commit -m "Update to X.Y.Z" && git push
   ```

4. Copy the new `sha256sums` into `packaging/kyko/PKGBUILD` too, regenerate
   `packaging/kyko/.SRCINFO` (`cd packaging/kyko && makepkg --printsrcinfo > .SRCINFO`), and commit
   it to `main`, so `packaging/` mirrors the AUR. This commit does not release anything: the
   version already has a tag.
5. Check the install: `yay -S kyko` on a clean system or VM.
