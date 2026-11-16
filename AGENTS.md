# Agent notes for kyko

A drop-down terminal for the uQ desktop, in the manner of Yakuake. Rust workspace, edition 2024. `kykod` (`apps/kykod`) holds the shells and keeps them running with no window open; `kyko` (`apps/kyko`) will be the window and for now only prints its version. The design is `docs/superpowers/specs/2026-11-10-kyko-design.md`, with one plan per slice in `docs/superpowers/plans/` and check results in `docs/notes/`.

## Verify

CI (`.github/workflows/ci.yml`) runs these in an `archlinux` container as an unprivileged user:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked                 # the end-to-end tests run `less`; install it
cargo deny check advisories licenses
for t in tests/shell/*.sh; do bash "$t"; done   # version_test.sh, release_needed_test.sh
scripts/build-local-pkg.sh                      # packs HEAD with git archive, runs makepkg; output in target/pkg/
```

- `KYKO_RANDOM_CASES=<n>` changes how many cases kyko-core's random session tests run. `kyko-core` tests are built at `opt-level = 2` (root `Cargo.toml`) because unoptimised they take half a minute; don't remove that.
- `cargo test -p kykod --test modes_bench -- --ignored --nocapture` prints throughput. It is a measurement, not a pass/fail check.
- `scripts/build-local-pkg.sh` builds HEAD only. Uncommitted changes are not in the package.
- `tests/shell/release_needed_test.sh` ends with a read-only run of `scripts/release-needed.sh` on this repository, so it needs full history and tags (not a shallow clone).

## Release

Read `docs/releasing.md`. This repo differs from the user's usual tag-and-push recipe:

- **Do not create or push a tag.** A push to `main` whose `Cargo.toml` version has no tag yet releases by itself: `release.yml` runs CI, builds and installs the package, then creates the tag `vX.Y.Z` and the GitHub Release. Every other push to `main` ends green with `skip`.
- So merging a version bump to `main` is the release. Do it only when the release gate in `docs/releasing.md` is met.
- Steps: move the `[Unreleased]` notes in `CHANGELOG.md` under `## [X.Y.Z] - YYYY-MM-DD` (an empty or missing section refuses the release), run `scripts/bump-version.sh X.Y.Z`, run `bash tests/shell/version_test.sh`, commit `release: vX.Y.Z`, merge.
- `scripts/bump-version.sh` sets the version in `Cargo.toml`, `Cargo.lock` and `packaging/kyko/PKGBUILD` (`pkgrel=1`) and regenerates `packaging/kyko/.SRCINFO`. Never edit those by hand. A release candidate's `pkgver` has no dash: `1.0.0-rc.1` is `1.0.0rc1`.
- Everything below 1.0.0 is published as a GitHub pre-release.
- AUR: the `aur` job pushes to `ssh://aur@aur.archlinux.org/kyko.git` only when the `AUR_SSH_KEY` secret exists. When it is skipped, push the `PKGBUILD` and `.SRCINFO` from the run's `aur` artifact by hand. Either way, copy those two files into `packaging/kyko/` and commit them afterwards: the workflow does not commit the new `sha256sums` to `main`. That commit releases nothing.
- If the run fails after the tag was pushed, a re-run only skips. Recovery (delete the partial release and the tag, then re-run) is in `docs/releasing.md`.

## Gotchas

- kykod's tests end shells whose children are re-parented to PID 1. In a container without an init (`docker run` without `--init`) those become zombies for the rest of the run; CI passes `--init` for this reason.
- `crates/kyko-core/themes/*.json` and its `CREDITS.md` are generated from uQ by `scripts/sync-uq-themes.sh <uq checkout>` and compiled into kyko-core. Don't edit them by hand; re-running the script is a deliberate commit of its own.
- `deny.toml` fails on unmaintained and yanked crates, transitive ones included, and allows only the listed permissive licences. A new dependency can fail CI for either reason; check with `cargo deny check advisories licenses` before adding one.
- `packaging/kykod.service` is installed but not enabled. Don't enable or start it from the package.
- `scripts/release-needed.sh` exits 3 with a one-line `error:` when a release is refused; the messages and fixes are listed in `docs/releasing.md`.
