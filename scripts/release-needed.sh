#!/usr/bin/env bash
# Decides whether the current commit should be released. Reads the version from Cargo.toml and
# the repository's v* tags, prints exactly one line and sets the exit code:
#
#   scripts/release-needed.sh [--root DIR]
#
#   release X.Y.Z[-rc.N] prerelease|final   exit 0  no tag vX.Y.Z yet, and it is above every v* tag
#   skip X.Y.Z                              exit 0  tag vX.Y.Z is HEAD or one of its ancestors
#   error: <reason>                         exit 3  the tag is on another history, the version is
#                                                   lower than an existing tag, or CHANGELOG.md has
#                                                   no section for it (checked only for `release`)
#
# `prerelease` when the major version is 0 or the version is a release candidate (-rc.N);
# otherwise `final`. Versions compare with sort -V after writing '-rc.' as '~rc', so a candidate
# sorts before its final (1.0.0-rc.1 < 1.0.0). Needs full history and tags (fetch-depth: 0).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
if [ "${1:-}" = "--root" ]; then
    [ $# -ge 2 ] || { echo "usage: $0 [--root DIR]" >&2; exit 2; }
    root=$(cd "$2" && pwd)
    shift 2
fi
[ $# -eq 0 ] || { echo "usage: $0 [--root DIR]" >&2; exit 2; }

fail() { echo "error: $1"; exit 3; }

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-rc\.[1-9][0-9]*)?$'
[[ $version =~ $re ]] || { echo "release-needed: no valid version in $root/Cargo.toml" >&2; exit 1; }
tag=v$version

# Same history: the tag exists and is HEAD or an ancestor of it.
if git -C "$root" rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    if git -C "$root" merge-base --is-ancestor "refs/tags/$tag^{commit}" HEAD; then
        echo "skip $version"
        exit 0
    fi
    fail "tag $tag exists at another history"
fi

# Newest existing version tag (tags that are not vX.Y.Z[-rc.N] are ignored).
highest=$(git -C "$root" tag --list 'v*' | sed 's/^v//' | grep -E "$re" | sed 's/-rc\./~rc/' | sort -V | tail -n 1 | sed 's/~rc/-rc./' || true)
if [ -n "$highest" ]; then
    top=$(printf '%s\n%s\n' "${version/-rc./~rc}" "${highest/-rc./~rc}" | sort -V | tail -n 1)
    [ "$top" = "${version/-rc./~rc}" ] || fail "$version is lower than v$highest"
fi

if ! "$(dirname "$0")/changelog-section.sh" "$version" "$root/CHANGELOG.md" >/dev/null 2>&1; then
    fail "no CHANGELOG section for $version"
fi

if [ "${version%%.*}" = 0 ] || [[ $version == *-rc.* ]]; then
    echo "release $version prerelease"
else
    echo "release $version final"
fi
