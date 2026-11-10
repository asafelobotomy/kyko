#!/usr/bin/env bash
# Prints the body of one CHANGELOG.md section (without its `## [...]` heading), for release notes.
#
#   scripts/changelog-section.sh <version|Unreleased> [CHANGELOG]
#
# Fails (exit 1) when the section is missing or has no text.
set -euo pipefail
[ $# -ge 1 ] && [ $# -le 2 ] || { echo "usage: $0 <version|Unreleased> [CHANGELOG]" >&2; exit 2; }
want=${1#v}
file=${2:-$(cd "$(dirname "$0")/.." && pwd)/CHANGELOG.md}
[ -f "$file" ] || { echo "changelog-section: missing $file" >&2; exit 1; }

body=$(awk -v want="$want" '
    /^## \[/ {
        if (inside) exit
        head = $0
        sub(/^## \[/, "", head); sub(/\].*$/, "", head)
        if (head == want) { inside = 1; next }
    }
    # Link references at the bottom ([0.1.0]: https://...) end a section too.
    inside && /^\[[^]]+\]: / { exit }
    inside { print }
' "$file" | sed -e '/./,$!d')   # drop leading blank lines; $() drops trailing ones

if [ -z "${body//[[:space:]]/}" ]; then
    echo "changelog-section: no '## [$want]' section with text in $file" >&2
    exit 1
fi
printf '%s\n' "$body"
