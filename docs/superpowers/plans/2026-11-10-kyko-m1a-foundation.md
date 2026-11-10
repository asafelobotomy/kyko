# kyko Milestone 1a (workspace, packaging, CI, release on version bump) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A real Cargo workspace whose only feature is `kyko --version`, packaged for Arch, built and tested by CI on every push, and released automatically when the version on `main` changes.

**Architecture:** Mirrors uQ's release tooling (`~/Projects/uq/scripts/`, `packaging/uq/`, `.github/workflows/`), with one difference: the tag is created by the release workflow, not by hand. Every decision the workflow makes lives in a shell script that is tested locally; the workflow files only call those scripts.

**Tech Stack:** Rust stable (edition 2024, resolver 3), `clap` 4, GitHub Actions in an `archlinux:latest` container, `makepkg`, `cargo-deny`, the `gh` CLI for releases.

**Spec:** `docs/superpowers/specs/2026-11-10-kyko-design.md` (§8 defines this slice; §2 gives the workspace layout; §2.11 the supply-chain rules).

Milestone 1 is split into four plans, each ending in an installable version:

| Slice | Delivers | Version |
|---|---|---|
| **1a (this plan)** | Workspace, packaging, CI, release on bump | 0.0.1 |
| 1b | `kyko-proto`, `kyko-term`, `kykod` (no window) | 0.0.2 |
| 1c | The window: glyph-atlas renderer, layer shell, keyboard, toggle over D-Bus, both shapes | 0.0.3 |
| 1d | Config file, theme sources, Hyprland checks in the uQ VM, open risks 1 to 6 closed | 0.1.0 |

## Global Constraints

- Workspace members: `crates/kyko-core`, `apps/kykod`, `apps/kyko`. Edition 2024, `resolver = "3"`, license MIT, repository `https://github.com/asafelobotomy/kyko`. The workspace `version` in the root `Cargo.toml` is the only version.
- `rust-toolchain.toml`: channel `stable`, components `rustfmt`, `clippy` (as uQ).
- Tags are `vX.Y.Z` or `vX.Y.Z-rc.N`. `pkgver` is the version with `-rc.` written `rc` (`1.0.0-rc.1` → `1.0.0rc1`); the PKGBUILD maps it back with `_tag=${pkgver/rc/-rc.}`.
- Versions below 1.0.0 and every release candidate are GitHub pre-releases.
- A push to `main` whose version already has a tag releases nothing.
- The AUR package and the release build from the release asset `kyko-X.Y.Z.tar.gz` made by `git archive`, never from GitHub's `/archive/` tarball.
- GitHub Actions are pinned by full commit SHA with the version in a comment. Reuse uQ's pins where the action is the same: `actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4.3.1`, `actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2`, `actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093 # v4.3.0`. Any other action: look up the SHA of its latest release tag with `gh api` and pin that.
- Workflows default to `permissions: contents: read`; only the release job widens them.
- `Cargo.lock` is committed. CI runs `cargo deny check advisories licenses`.
- `kyko-core` carries `#![forbid(unsafe_code)]`.
- Package dependencies in this slice: none beyond `gcc-libs`; `makedepends=(cargo)`. Qt dependencies arrive with slice 1c, when something links Qt.
- uQ's scripts are MIT and by the same author: copy and adapt them, keeping their behaviour and comments, changing names (`uq` → `kyko`, `packaging/uq` → `packaging/kyko`).
- Commit messages end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.
- Creating the GitHub repository, pushing, and anything touching the AUR need the user's explicit go-ahead (Task 6).

## Review Focus

1. **A release-candidate version** (`0.2.0-rc.1`): `pkgver` must become `0.2.0rc1`, the tag `v0.2.0-rc.1`, and the release a pre-release. Tests in Tasks 2 and 3.
2. **A push that does not change the version:** nothing is tagged or published, and the workflow ends green. Test in Task 3.
3. **A version lower than, or equal to but different in content from, an existing tag** (a bump backwards, or a tag someone made by hand at another commit): the release refuses with a clear message and publishes nothing. Test in Task 3.
4. **No CHANGELOG section for the version:** the release fails before a tag is created. Test in Task 3.
5. **No `AUR_SSH_KEY` secret, as on a fork or before the user adds it:** the release still succeeds and leaves the AUR files as an artifact. Covered by the workflow's own condition in Task 5; cannot be tested locally.

---

### Task 1: Workspace and `--version`

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `LICENSE` (copy `~/Projects/uq/LICENSE`), `README.md`, `CHANGELOG.md`, `crates/kyko-core/Cargo.toml`, `crates/kyko-core/src/lib.rs`, `apps/kykod/Cargo.toml`, `apps/kykod/src/main.rs`, `apps/kyko/Cargo.toml`, `apps/kyko/src/main.rs`
- Delete: `spike/` (it stays on branch `spike/m0` and in history)
- Modify: `.gitignore` (`target/`, `.superpowers/`, `dist/`)
- Test: `crates/kyko-core/src/lib.rs` (inline), `apps/kyko/tests/cli.rs`, `apps/kykod/tests/cli.rs`

**Interfaces:**
- Produces: `kyko_core::VERSION: &str` (`env!("CARGO_PKG_VERSION")`). Binaries `kyko` and `kykod`; each prints `<name> <version>` for `--version` and exits 0; each exits 2 with clap's usage for an unknown argument. With no arguments `kyko` prints `kyko: nothing to do yet (the window arrives in 0.0.3)` to stderr and exits 1; `kykod` prints the same with `the session holder arrives in 0.0.2`.

- [ ] **Step 1: Branch.** `git switch -c m1a-foundation spike/m0`, then `git rm -r -q spike` and commit `chore: remove the spike (kept on spike/m0)`.

- [ ] **Step 2: Write the failing tests**

```rust
// crates/kyko-core/src/lib.rs
#[test] fn version_is_the_workspace_version() {
    assert_eq!(VERSION, "0.0.1");
}

// apps/kyko/tests/cli.rs  (apps/kykod/tests/cli.rs is the same with "kykod")
use std::process::Command;
fn bin() -> Command { Command::new(env!("CARGO_BIN_EXE_kyko")) }
#[test] fn version_flag_prints_name_and_version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), "kyko 0.0.1");
}
#[test] fn unknown_flag_exits_2() {
    assert_eq!(bin().arg("--nope").output().unwrap().status.code(), Some(2));
}
#[test] fn no_arguments_exits_1_with_a_message() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().contains("nothing to do yet"));
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test --workspace`
Expected: fails to compile (no crates yet).

- [ ] **Step 4: Implement** the workspace (`version = "0.0.1"` under `[workspace.package]`), `kyko-core`, and the two binaries with `clap` (`#[command(version, about)]`). `CHANGELOG.md` uses Keep a Changelog headings with `## [Unreleased]` and `## [0.0.1] - 2026-11-10` ("Workspace, packaging and CI; no features yet."). `README.md`: what kyko is (two sentences from spec §1), that nothing is usable yet, and how to build.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`
Expected: 7 tests pass, no warnings.

- [ ] **Step 6: Commit** `feat: workspace with kyko and kykod --version`.

### Task 2: Package and version tooling

**Files:**
- Create: `packaging/kyko/PKGBUILD`, `packaging/kyko/.SRCINFO`, `scripts/bump-version.sh`, `scripts/build-local-pkg.sh`
- Test: `tests/shell/version_test.sh`

**Interfaces:**
- Produces: `scripts/bump-version.sh [--root DIR] X.Y.Z[-rc.N]` (sets `Cargo.toml`, the workspace members in `Cargo.lock`, `pkgver`, `pkgrel=1`, regenerates `.SRCINFO`; exits 2 on a malformed version). `scripts/build-local-pkg.sh [makepkg args]` builds `HEAD` into `target/pkg/` (as uQ's, single package). `tests/shell/version_test.sh` exits non-zero on any failure.

- [ ] **Step 1: Write the failing test.** Copy `~/Projects/uq/tests/shell/version_test.sh` to `tests/shell/version_test.sh`, change `packaging/uq` to `packaging/kyko`, and drop its changelog-section part (Task 3 adds it back). It must check: Cargo and `pkgver` agree; every `packaging/*/.SRCINFO` is current; and `bump-version.sh` on a temporary copy maps `1.2.3` → `pkgver=1.2.3`, `1.2.3-rc.4` → `pkgver=1.2.3rc4`, resets `pkgrel` to 1, updates all three workspace members in `Cargo.lock`, and exits 2 for `v1.2.3`, `1.2`, `01.2.3` and `1.2.3-rc.0`.

- [ ] **Step 2: Run to verify it fails**

Run: `bash tests/shell/version_test.sh`
Expected: `FAIL` lines for the missing PKGBUILD and script; exit 1.

- [ ] **Step 3: Implement.**
  - `PKGBUILD`: copy uQ's shape. `pkgname=kyko`, `pkgver=0.0.1`, `pkgrel=1`, `pkgdesc="Drop-down terminal for uQ and other Wayland desktops"`, `depends=(gcc-libs)`, `makedepends=(cargo)`, `options=(!lto)`, the release-asset `source` line with `kyko`, `sha256sums=('SKIP')` until the first release. `build()`: `cargo build --frozen --release -p kyko -p kykod`. `check()`: `cargo test --frozen --release --workspace`. `package()`: install `target/release/kyko` and `kykod` to `/usr/bin`, `LICENSE` to `/usr/share/licenses/kyko/`, `README.md` to `/usr/share/doc/kyko/`.
  - `bump-version.sh` and `build-local-pkg.sh`: copy uQ's and rename; `build-local-pkg.sh` loses its `--package` option.
  - `.SRCINFO`: `(cd packaging/kyko && makepkg --printsrcinfo > .SRCINFO)`.

- [ ] **Step 4: Run to verify it passes**

Run: `bash tests/shell/version_test.sh && scripts/build-local-pkg.sh && pacman -Qlp target/pkg/kyko-0.0.1-1-x86_64.pkg.tar.zst`
Expected: the test prints no `FAIL`; the package lists `/usr/bin/kyko`, `/usr/bin/kykod`, the licence and the README.

- [ ] **Step 5: Commit** `build: Arch package and version tooling`.

### Task 3: Release decisions as scripts

**Files:**
- Create: `scripts/release-needed.sh`, `scripts/changelog-section.sh`
- Modify: `tests/shell/version_test.sh` (add the changelog part back)
- Test: `tests/shell/release_needed_test.sh`

**Interfaces:**
- Produces:
  - `scripts/changelog-section.sh X.Y.Z[-rc.N] [FILE]`: prints the body of `## [X.Y.Z]` from `CHANGELOG.md`; exits 1 with `changelog-section: no section for X.Y.Z` if it is missing or empty (copy uQ's).
  - `scripts/release-needed.sh [--root DIR]`: reads the version from `Cargo.toml` and the repository's tags. Prints exactly one line to stdout and sets the exit code:

    | Situation | stdout | exit |
    |---|---|---|
    | No tag `v<version>` exists, and `<version>` is greater than every existing `v*` tag | `release <version> <prerelease\|final>` | 0 |
    | Tag `v<version>` exists and points at `HEAD` or an ancestor of `HEAD` | `skip <version>` | 0 |
    | Tag `v<version>` exists but is not an ancestor of `HEAD` | `error: tag v<version> exists at another history` | 3 |
    | `<version>` is lower than an existing tag | `error: <version> is lower than v<highest>` | 3 |
    | `CHANGELOG.md` has no section for `<version>` (checked only when the answer would be `release`) | `error: no CHANGELOG section for <version>` | 3 |

    `prerelease` when the major version is 0 or the version has `-rc.`; otherwise `final`. Versions compare with `sort -V` after writing `-rc.` as `~rc` so a candidate sorts before its final.

- [ ] **Step 1: Write the failing test** `tests/shell/release_needed_test.sh`. It builds a temporary git repository per case (a `Cargo.toml` with `version = "…"`, a `CHANGELOG.md`, commits and tags) and asserts stdout and exit code for:

  | Case | Setup | Expect |
  |---|---|---|
  | first release | version 0.0.1, no tags, section present | `release 0.0.1 prerelease`, 0 |
  | unchanged | version 0.0.1, tag v0.0.1 at HEAD | `skip 0.0.1`, 0 |
  | later commit, same version | tag v0.0.1 one commit back | `skip 0.0.1`, 0 |
  | bump | version 0.0.2, tag v0.0.1, section present | `release 0.0.2 prerelease`, 0 |
  | release candidate | version 1.0.0-rc.1, tag v0.9.0 | `release 1.0.0-rc.1 prerelease`, 0 |
  | final after candidate | version 1.0.0, tags v0.9.0 and v1.0.0-rc.1 | `release 1.0.0 final`, 0 |
  | backwards | version 0.0.1, tag v0.0.2 | `error: 0.0.1 is lower than v0.0.2`, 3 |
  | foreign tag | tag v0.0.1 on an unrelated branch | `error: tag v0.0.1 exists at another history`, 3 |
  | no changelog | version 0.0.2, tag v0.0.1, no `## [0.0.2]` | `error: no CHANGELOG section for 0.0.2`, 3 |

- [ ] **Step 2: Run to verify it fails**

Run: `bash tests/shell/release_needed_test.sh`
Expected: every case fails (script missing); exit 1.

- [ ] **Step 3: Implement** both scripts.

- [ ] **Step 4: Run to verify it passes**

Run: `for t in tests/shell/*.sh; do bash "$t" || exit 1; done && scripts/release-needed.sh`
Expected: no `FAIL`; the last line is `release 0.0.1 prerelease`.

- [ ] **Step 5: Commit** `build: release decision scripts with tests`.

### Task 4: CI

**Files:**
- Create: `.github/workflows/ci.yml`, `deny.toml`

**Interfaces:**
- Consumes: the scripts and tests from Tasks 2 and 3.
- Produces: workflow `CI`, triggered on `push` (branches `**`), `pull_request` and `workflow_call`, with one job `test` in `archlinux:latest`; and a workflow artifact `kyko-pkg` holding the built `.pkg.tar.zst`.

- [ ] **Step 1: Write `deny.toml`:** advisories deny vulnerabilities and unmaintained crates; licenses allow `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`, `Unicode-3.0`, `Zlib`; everything else denied.

Run: `cargo install --locked cargo-deny && cargo deny check advisories licenses`
Expected: `advisories ok, licenses ok`. If a licence is refused, add it only if it is a permissive licence compatible with MIT, and say which crate needed it in the commit message.

- [ ] **Step 2: Write `ci.yml`.** Steps, in this order: install `rust git base-devel cargo-deny` with pacman; checkout (pinned, `persist-credentials: false`, `fetch-depth: 0` so tags are visible); create user `tester` and `chown` the workspace (makepkg refuses root); then as `tester`: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test --workspace --locked`; `cargo deny check advisories licenses`; every `tests/shell/*.sh`; `scripts/build-local-pkg.sh`; then as root `pacman -U --noconfirm target/pkg/kyko-*.pkg.tar.zst && kyko --version && kykod --version`; upload `target/pkg/*.pkg.tar.zst` as `kyko-pkg`.

- [ ] **Step 3: Verify the workflow's commands locally in the same container image.**

Run: `podman run --rm -v "$PWD:/src:ro" archlinux:latest bash -c 'pacman -Syu --noconfirm --needed rust git base-devel cargo-deny >/dev/null && useradd -m tester && cp -r /src /home/tester/kyko && chown -R tester /home/tester/kyko && su tester -c "cd ~/kyko && cargo test --workspace --locked && for t in tests/shell/*.sh; do bash \$t || exit 1; done && scripts/build-local-pkg.sh" && pacman -U --noconfirm /home/tester/kyko/target/pkg/kyko-*.pkg.tar.zst && kyko --version'` (use `docker` if `podman` is absent; install one with the user's standing permission to install dependencies).
Expected: ends with `kyko 0.0.1`.

- [ ] **Step 4: Lint the workflow.**

Run: `pacman -Q actionlint >/dev/null 2>&1 || run0 pacman -S --noconfirm actionlint; actionlint .github/workflows/ci.yml`
Expected: no output.

- [ ] **Step 5: Commit** `ci: format, lint, test, audit and package on every push`.

### Task 5: Release workflow

**Files:**
- Create: `.github/workflows/release.yml`, `docs/releasing.md`

**Interfaces:**
- Consumes: `scripts/release-needed.sh`, `scripts/changelog-section.sh`, workflow `CI` (by `workflow_call`).
- Produces: workflow `Release`, triggered on `push` to `main` only. Jobs:
  1. `decide` (`contents: read`): checkout with `fetch-depth: 0`, run `scripts/release-needed.sh`, expose outputs `action` (`release` or `skip`), `version`, `kind`. Exit code 3 fails the run.
  2. `ci`: `uses: ./.github/workflows/ci.yml`, only when `action == release`.
  3. `publish` (needs `decide` and `ci`; `permissions: contents: write, id-token: write, attestations: write`; `archlinux:latest`): 
     - `git archive --format=tar.gz --prefix=kyko-$V/ -o dist/kyko-$V.tar.gz HEAD` and its `.sha256`;
     - build the package from that tarball as user `builder` (a copy of the PKGBUILD with `source` pointing at the local tarball and its real checksum), install it, run `kyko --version` and assert it prints `kyko $V`;
     - attest the tarball and the package (`actions/attest-build-provenance`, pinned);
     - create and push the tag `v$V` with `git tag -a` as `github-actions[bot]`;
     - `gh release create v$V` with `--notes-file` from `changelog-section.sh`, `--prerelease` when `kind == prerelease`, and the tarball, `.sha256` and `.pkg.tar.zst` as assets;
     - write `aur/PKGBUILD` (with the published checksum) and `aur/.SRCINFO`, upload them as artifact `aur`, and put the checksum in the job summary.
  4. `aur` (needs `publish`; runs only `if: ${{ secrets.AUR_SSH_KEY != '' }}` evaluated through a job-level `env`): clone `ssh://aur@aur.archlinux.org/kyko.git`, copy the two files, commit `Update to $V`, push.
- Nothing is tagged or published before the package built from the tarball has installed and reported the right version.

- [ ] **Step 1: Write `release.yml`** to the interface above. The `publish` job's tag-and-release steps come last, after every check.

- [ ] **Step 2: Lint.**

Run: `actionlint .github/workflows/release.yml`
Expected: no output.

- [ ] **Step 3: Dry-run the publish job's build locally.**

Run: `V=0.0.1; mkdir -p dist && git archive --format=tar.gz --prefix=kyko-$V/ -o dist/kyko-$V.tar.gz HEAD && sha256sum dist/kyko-$V.tar.gz` and then build from it exactly as the job does (same container command as Task 4 Step 3, with the tarball as source).
Expected: the package installs and `kyko --version` prints `kyko 0.0.1`.

- [ ] **Step 4: Write `docs/releasing.md`:** how a release happens (bump with `scripts/bump-version.sh`, write the CHANGELOG section, merge to `main`; the workflow does the rest), the release gate (CI green, CHANGELOG written, live checklist for the milestone done), what each failure message from `release-needed.sh` means and how to recover, how to add `AUR_SSH_KEY`, and the manual AUR steps for when it is absent (as uQ's `docs/releasing.md`).

- [ ] **Step 5: Commit** `ci: release on version bump`.

### Task 6: Repository and first release

**Files:**
- Modify: `packaging/kyko/PKGBUILD` and `.SRCINFO` (the real checksum, after the release)

- [ ] **Step 1: STOP and ask the user.** Everything here is outward-facing. Ask for: the repository name (default `asafelobotomy/kyko`), public or private (AUR packages need public source), and whether to merge `m1a-foundation` into `main` and push. Do not create the repository, push, or touch the AUR without that answer.

- [ ] **Step 2: On the user's go-ahead,** merge `m1a-foundation` into `main` (fast-forward), create the repository with `gh repo create`, and push `main`.

- [ ] **Step 3: Watch the first runs.**

Run: `gh run list --limit 5` and `gh run view <id> --log-failed` for any failure.
Expected: `CI` green; `Release` green with a pre-release `v0.0.1` holding `kyko-0.0.1.tar.gz`, its `.sha256` and `kyko-0.0.1-1-x86_64.pkg.tar.zst`; job `aur` skipped.

- [ ] **Step 4: Check the published package on this machine.**

Run: `gh release download v0.0.1 -p '*.pkg.tar.zst' -D "$(mktemp -d)"`, then `pacman -Qip` on it and `gh attestation verify` on the tarball.
Expected: version `0.0.1-1`; the attestation verifies. Installing it system-wide is the user's choice.

- [ ] **Step 5: Push a no-op commit** (the PKGBUILD checksum from the release, per `docs/releasing.md`) and confirm the next `Release` run ends with `skip 0.0.1` and publishes nothing.

- [ ] **Step 6: Report** the release URL, what was verified, and that the AUR push is off until the user adds `AUR_SSH_KEY`.
