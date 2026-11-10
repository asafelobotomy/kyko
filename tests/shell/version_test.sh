#!/usr/bin/env bash
# Release plumbing checks:
#   - Cargo.toml's workspace version and packaging/kyko's pkgver agree ('-rc.N' is 'rcN' in pkgver);
#   - every packaging/*/.SRCINFO matches `makepkg --printsrcinfo` of its PKGBUILD;
#   - scripts/bump-version.sh maps and refuses versions correctly (run on a temporary copy), and
#     after a bump no file under crates/ or apps/ still names the old version;
#   - scripts/changelog-section.sh extracts exactly one section.
# No diff/cmp: CI's container has no diffutils.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
fail=0
bad() { echo "FAIL: $*"; fail=1; }

cargo_version() { sed -n 's/^version = "\(.*\)"$/\1/p' "$1"; }
pkgbuild_var() { sed -n "s/^$1=//p" "$2"; }

# 1. Versions agree.
cv=$(cargo_version "$root/Cargo.toml")
pv=$(pkgbuild_var pkgver "$root/packaging/kyko/PKGBUILD")
[ -n "$cv" ] || bad "no workspace version in Cargo.toml"
[ "${cv/-rc./rc}" = "$pv" ] || bad "Cargo.toml version $cv does not match PKGBUILD pkgver $pv (want ${cv/-rc./rc})"

# 2. .SRCINFO is current. makepkg refuses to run as root.
if command -v makepkg >/dev/null 2>&1 && [ "$(id -u)" != 0 ]; then
    for d in "$root"/packaging/*/; do
        [ -f "$d/PKGBUILD" ] || continue
        n=$(basename "$d")
        if [ ! -f "$d/.SRCINFO" ]; then bad "packaging/$n/.SRCINFO missing"; continue; fi
        want=$(cd "$d" && makepkg --printsrcinfo 2>/dev/null) || { bad "makepkg --printsrcinfo failed in packaging/$n"; continue; }
        [ "$want" = "$(cat "$d/.SRCINFO")" ] \
            || bad "packaging/$n/.SRCINFO is stale: run (cd packaging/$n && makepkg --printsrcinfo > .SRCINFO)"
    done
    have_makepkg=1
else
    echo "skip: .SRCINFO freshness (makepkg missing or running as root)"
    have_makepkg=0
fi

# 3. bump-version.sh on a copy.
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fresh_copy() {
    rm -rf "$tmp/r"
    mkdir -p "$tmp/r/packaging/kyko"
    cp "$root/Cargo.toml" "$root/Cargo.lock" "$tmp/r/"
    # The sources too: a version written into them by hand would survive a bump (checked below).
    # With crates/ present, bump-version.sh also runs its `cargo update -w --offline` check.
    cp -r "$root/crates" "$root/apps" "$tmp/r/"
    cp "$root/packaging/kyko/PKGBUILD" "$tmp/r/packaging/kyko/"
    sed -i 's/^pkgrel=.*/pkgrel=7/' "$tmp/r/packaging/kyko/PKGBUILD"
}
# Version lines of registry packages (the ones with a source), which a bump must not touch.
registry_versions() { awk '/^\[\[package\]\]$/{if(s)print v; v=""; s=0} /^version = /{v=$0} /^source = /{s=1} END{if(s)print v}' "$1"; }
member_versions() { awk '/^\[\[package\]\]$/{if(n!=""&&!s)print n, v; n=""; v=""; s=0} /^name = /{n=$3} /^version = /{v=$3} /^source = /{s=1} END{if(n!=""&&!s)print n, v}' "$1"; }

check_bump() { # version expected-pkgver
    fresh_copy
    local before out
    before=$(registry_versions "$tmp/r/Cargo.lock")
    out=$("$root/scripts/bump-version.sh" --root "$tmp/r" "$1" 2>&1) || { bad "bump $1 failed: $out"; return; }
    [ "$(cargo_version "$tmp/r/Cargo.toml")" = "$1" ] || bad "bump $1: Cargo.toml has $(cargo_version "$tmp/r/Cargo.toml")"
    [ "$(pkgbuild_var pkgver "$tmp/r/packaging/kyko/PKGBUILD")" = "$2" ] || bad "bump $1: pkgver is $(pkgbuild_var pkgver "$tmp/r/packaging/kyko/PKGBUILD"), want $2"
    [ "$(pkgbuild_var pkgrel "$tmp/r/packaging/kyko/PKGBUILD")" = 1 ] || bad "bump $1: pkgrel not reset to 1"
    local members
    members=$(member_versions "$tmp/r/Cargo.lock")
    [ -n "$members" ] || bad "bump $1: no workspace members found in Cargo.lock"
    while read -r name ver; do
        [ "$ver" = "\"$1\"" ] || bad "bump $1: Cargo.lock $name is $ver"
    done <<<"$members"
    [ "$(registry_versions "$tmp/r/Cargo.lock")" = "$before" ] || bad "bump $1: changed registry package versions in Cargo.lock"
    # The bump only edits Cargo.toml and Cargo.lock, so the sources must take the version from
    # there (env!("CARGO_PKG_VERSION")) and never spell it out. -w: 0.0.1 does not match 10.0.1.
    local stale
    stale=$(cd "$tmp/r" && grep -rlwF -- "$cv" crates apps)
    [ -z "$stale" ] || bad "bump $1: still names the old version $cv: ${stale//$'\n'/ }"
    if [ "$have_makepkg" = 1 ]; then
        grep -q "^	pkgver = $2\$" "$tmp/r/packaging/kyko/.SRCINFO" 2>/dev/null || bad "bump $1: .SRCINFO not regenerated with pkgver $2"
        grep -q "^	source = kyko-${1}.tar.gz::https://.*/releases/download/v${1}/kyko-${1}.tar.gz\$" "$tmp/r/packaging/kyko/.SRCINFO" \
            || bad "bump $1: .SRCINFO source does not point at the v$1 release asset"
    fi
}
check_bump 1.2.3 1.2.3
check_bump 0.2.0 0.2.0
check_bump 1.0.0-rc.1 1.0.0rc1
check_bump 2.10.0-rc.12 2.10.0rc12

for v in 1.2 v1.2.3 1.2.3.4 1.2.3-beta.1 1.2.3-rc 1.2.3-rc.0 1.2.3-rc1 01.2.3 1.2.3rc1 '' ' 1.2.3' '1.2.3 '; do
    fresh_copy
    toml=$(cat "$tmp/r/Cargo.toml"); pb=$(cat "$tmp/r/packaging/kyko/PKGBUILD"); lock=$(cat "$tmp/r/Cargo.lock")
    if "$root/scripts/bump-version.sh" --root "$tmp/r" "$v" >/dev/null 2>&1; then
        bad "bump accepted malformed version '$v'"
    fi
    [ "$toml" = "$(cat "$tmp/r/Cargo.toml")" ] && [ "$pb" = "$(cat "$tmp/r/packaging/kyko/PKGBUILD")" ] \
        && [ "$lock" = "$(cat "$tmp/r/Cargo.lock")" ] || bad "refused bump '$v' still changed files"
done

# 4. changelog-section.sh.
cat >"$tmp/CHANGELOG.md" <<'EOF'
# Changelog

## [Unreleased]

- next

## [1.0.0-rc.1] - 2026-01-02

### Added

- rc thing

## [0.2.0] - 2026-01-01

- two

[Unreleased]: https://example.invalid/compare/v1.0.0-rc.1...HEAD
[1.0.0-rc.1]: https://example.invalid/releases/tag/v1.0.0-rc.1
EOF
cs=$root/scripts/changelog-section.sh
[ "$("$cs" 1.0.0-rc.1 "$tmp/CHANGELOG.md")" = "$(printf '### Added\n\n- rc thing')" ] || bad "changelog-section 1.0.0-rc.1: $("$cs" 1.0.0-rc.1 "$tmp/CHANGELOG.md" 2>&1)"
[ "$("$cs" v1.0.0-rc.1 "$tmp/CHANGELOG.md")" = "$(printf '### Added\n\n- rc thing')" ] || bad "changelog-section does not strip a leading v"
[ "$("$cs" 0.2.0 "$tmp/CHANGELOG.md")" = "- two" ] || bad "changelog-section 0.2.0 ran past the section or into the links: $("$cs" 0.2.0 "$tmp/CHANGELOG.md" 2>&1)"
[ "$("$cs" Unreleased "$tmp/CHANGELOG.md")" = "- next" ] || bad "changelog-section Unreleased"
"$cs" 0.2 "$tmp/CHANGELOG.md" >/dev/null 2>&1 && bad "changelog-section matched a prefix (0.2)"
"$cs" 9.9.9 "$tmp/CHANGELOG.md" >/dev/null 2>&1 && bad "changelog-section succeeded for a missing version"
# The real changelog has the current version or an Unreleased section.
"$cs" "$cv" >/dev/null 2>&1 || "$cs" Unreleased >/dev/null 2>&1 || bad "CHANGELOG.md has neither [$cv] nor [Unreleased]"

[ "$fail" = 0 ] && echo "version ok"
exit "$fail"
