#!/usr/bin/env bash
# check-uq-theme.sh <pinned-version> <vendored file>...
# Drift check for vendored uq-theme files (uq_theme.rs, uqtheme.h, uqtheme.cpp): each file's
# `uq-theme X.Y.Z` header must be the pinned version and its contents exactly that release's
# (the checksums below), so a local edit shows up in CI. Copy it into a consuming repo as
# scripts/check-uq-theme.sh. Exit 0 when all match, 1 on any mismatch or missing file, 2 on bad usage.
set -u
declare -A SUMS=(
    [1.0.0/uq_theme.rs]=1d7f38189426099831438fda3541e42f02ab282f1df54bafaf54f7f9d89c9708
    [1.0.0/uqtheme.cpp]=735f513fd3a54ad7b11e0970e2d7aa906a895e843bdca40ac21d8ba533f6685a
    [1.0.0/uqtheme.h]=4b3717e1708c131c6f164a992b8f846f97d5fa0d6b655ba0c649984333847940
)
if [ $# -lt 2 ] || ! [[ $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "usage: check-uq-theme.sh <pinned-version X.Y.Z> <vendored file>..." >&2
    exit 2
fi
pinned=$1; shift
status=0
for file in "$@"; do
    name=$(basename "$file")
    if [ ! -r "$file" ]; then
        echo "check-uq-theme: cannot read $file" >&2; status=1; continue
    fi
    found=$(head -n 3 "$file" | grep -m1 -oE 'uq-theme [0-9]+\.[0-9]+\.[0-9]+' | cut -d' ' -f2)
    if [ "$found" != "$pinned" ]; then
        echo "check-uq-theme: $file is uq-theme ${found:-?}, pinned $pinned" >&2; status=1; continue
    fi
    want=${SUMS[$pinned/$name]:-}
    have=$(sha256sum "$file" | cut -d' ' -f1)
    if [ -z "$want" ]; then
        echo "check-uq-theme: this script does not know uq-theme $pinned $name; copy the newer script" >&2; status=1
    elif [ "$want" != "$have" ]; then
        echo "check-uq-theme: $file was edited (it is not uq-theme $pinned's $name); copy it again from uQ" >&2; status=1
    else
        echo "check-uq-theme: $file is uq-theme $found"
    fi
done
exit $status
