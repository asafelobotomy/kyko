#!/usr/bin/env bash
# Sets kyko's version everywhere it lives: the workspace version in Cargo.toml, the workspace
# members in Cargo.lock, and pkgver/pkgrel (plus .SRCINFO) in packaging/kyko/.
#
#   scripts/bump-version.sh [--root DIR] X.Y.Z[-rc.N]
#
# pacman forbids '-' in pkgver, so a release candidate maps to pkgver without the dash and dot:
#   Cargo.toml / git tag      PKGBUILD pkgver
#   1.0.0                     1.0.0
#   1.0.0-rc.1  (v1.0.0-rc.1) 1.0.0rc1
# vercmp sorts 1.0.0rc1 before 1.0.0, as wanted. The PKGBUILD maps it back to the tag with
# _tag=${pkgver/rc/-rc.}. pkgrel goes back to 1 on every bump.
#
# Cargo.lock is edited in place (only the version lines of the path packages, i.e. the
# workspace members), which is what `cargo update -w` would do, without needing the network
# or downloaded crates. When cargo is around, `cargo update -w --locked --offline` then checks
# that cargo agrees (it fails if it would change the lock file).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
if [ "${1:-}" = "--root" ]; then
    [ $# -ge 2 ] || { echo "usage: $0 [--root DIR] X.Y.Z[-rc.N]" >&2; exit 2; }
    root=$(cd "$2" && pwd)
    shift 2
fi
[ $# -eq 1 ] || { echo "usage: $0 [--root DIR] X.Y.Z[-rc.N]" >&2; exit 2; }
version=$1

re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.[1-9][0-9]*)?$'
if ! [[ $version =~ $re ]]; then
    echo "bump-version: '$version' is not X.Y.Z or X.Y.Z-rc.N (no leading 'v', no leading zeros)" >&2
    exit 2
fi
pkgver=${version/-rc./rc}

cargo_toml=$root/Cargo.toml
cargo_lock=$root/Cargo.lock
pkgbuild=$root/packaging/kyko/PKGBUILD
for f in "$cargo_toml" "$cargo_lock" "$pkgbuild"; do
    [ -f "$f" ] || { echo "bump-version: missing $f" >&2; exit 1; }
done
# Exactly one top-level `version =` line (the [workspace.package] one) and one pkgver/pkgrel.
[ "$(grep -c '^version = "' "$cargo_toml")" = 1 ] || { echo "bump-version: expected one top-level version line in $cargo_toml" >&2; exit 1; }
[ "$(grep -c '^pkgver=' "$pkgbuild")" = 1 ] && [ "$(grep -c '^pkgrel=' "$pkgbuild")" = 1 ] \
    || { echo "bump-version: expected one pkgver= and one pkgrel= line in $pkgbuild" >&2; exit 1; }

sed -i "s/^version = \".*\"\$/version = \"$version\"/" "$cargo_toml"

# In Cargo.lock, a [[package]] with no `source =` line is a path package: a workspace member.
tmp=$(mktemp "$cargo_lock.XXXXXX")
awk -v ver="$version" '
    function flush(   i) {
        for (i = 1; i <= n; i++) {
            if (!has_source && buf[i] ~ /^version = "/) buf[i] = "version = \"" ver "\""
            print buf[i]
        }
        n = 0; has_source = 0
    }
    /^\[\[package\]\]$/ { flush() }
    { buf[++n] = $0; if ($0 ~ /^source = "/) has_source = 1 }
    END { flush() }
' "$cargo_lock" >"$tmp"
cat "$tmp" >"$cargo_lock"
rm -f "$tmp"

sed -i -e "s/^pkgver=.*/pkgver=$pkgver/" -e 's/^pkgrel=.*/pkgrel=1/' "$pkgbuild"

if command -v makepkg >/dev/null 2>&1 && [ "$(id -u)" != 0 ]; then
    (cd "$root/packaging/kyko" && makepkg --printsrcinfo >.SRCINFO)
else
    echo "bump-version: makepkg unavailable (or running as root); regenerate packaging/kyko/.SRCINFO by hand" >&2
fi

if command -v cargo >/dev/null 2>&1 && [ -d "$root/crates" ]; then
    (cd "$root" && cargo update -w --locked --offline --quiet) \
        || { echo "bump-version: cargo disagrees with the edited Cargo.lock; run cargo update -w and check it" >&2; exit 1; }
fi

echo "version $version (pkgver $pkgver, pkgrel 1)"
