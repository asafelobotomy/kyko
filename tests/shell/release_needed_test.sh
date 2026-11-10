#!/usr/bin/env bash
# Checks scripts/release-needed.sh against throwaway git repositories, one per case: stdout and
# exit code for first release, skip, bump, release candidate, final, backwards version, a tag on
# foreign history, and a missing CHANGELOG section.
# No diff/cmp: CI's container has no diffutils.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
script=$root/scripts/release-needed.sh
fail=0
bad() { echo "FAIL: $*"; fail=1; }

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Git that never reads the user's or the system's configuration (no signing, hooks, templates or
# default branch from there), for the throwaway repositories and the read-only check at the end.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
# Quiet git on the current throwaway repository.
g() { git -C "$repo" -c commit.gpgsign=false -c tag.gpgsign=false "$@"; }

new_repo() { # name version [changelog-version]: one commit holding Cargo.toml and CHANGELOG.md
    repo=$tmp/$1
    mkdir -p "$repo"
    git init -q -b main "$repo"
    git -C "$repo" config user.name test
    git -C "$repo" config user.email test@example.invalid
    set_version "$2" "${3-$2}"
    g add -A
    g commit -q -m "version $2"
}
set_version() { # version changelog-version ('' for none)
    printf '[workspace.package]\nversion = "%s"\n' "$1" >"$repo/Cargo.toml"
    {
        printf '# Changelog\n\n## [Unreleased]\n\n'
        [ -z "$2" ] || printf '## [%s] - 2026-01-01\n\n- thing\n' "$2"
    } >"$repo/CHANGELOG.md"
}
commit() { g commit -q --allow-empty -m "$1"; }

check() { # case-name want-stdout want-exit
    local out code
    out=$("$script" --root "$repo" 2>/dev/null); code=$?
    [ "$out" = "$2" ] || bad "$1: stdout '$out', want '$2'"
    [ "$code" = "$3" ] || bad "$1: exit $code, want $3"
}

new_repo first 0.0.1
check "first release" "release 0.0.1 prerelease" 0

new_repo unchanged 0.0.1
g tag v0.0.1
check "unchanged" "skip 0.0.1" 0

new_repo later 0.0.1
g tag v0.0.1
commit more
check "later commit, same version" "skip 0.0.1" 0

new_repo bump 0.0.1
g tag v0.0.1
set_version 0.0.2 0.0.2; g add -A; g commit -q -m bump
check "bump" "release 0.0.2 prerelease" 0

new_repo rc 0.9.0
g tag v0.9.0
set_version 1.0.0-rc.1 1.0.0-rc.1; g add -A; g commit -q -m rc
check "release candidate" "release 1.0.0-rc.1 prerelease" 0

new_repo final 0.9.0
g tag v0.9.0
set_version 1.0.0-rc.1 1.0.0-rc.1; g add -A; g commit -q -m rc
g tag v1.0.0-rc.1
set_version 1.0.0 1.0.0; g add -A; g commit -q -m final
check "final after candidate" "release 1.0.0 final" 0

new_repo rcnew 1.1.0-rc.2
g tag v1.1.0-rc.1
check "second candidate" "release 1.1.0-rc.2 prerelease" 0

new_repo back 0.0.1
commit two
g tag v0.0.2
check "backwards" "error: 0.0.1 is lower than v0.0.2" 3

new_repo candidate-after-final 1.0.0-rc.2
g tag v1.0.0
check "candidate after its final" "error: 1.0.0-rc.2 is lower than v1.0.0" 3

new_repo foreign 0.0.1
g checkout -q -b other
commit elsewhere
g tag v0.0.1
g checkout -q main
commit onmain
check "foreign tag" "error: tag v0.0.1 exists at another history" 3

new_repo nolog 0.0.1
g tag v0.0.1
set_version 0.0.2 ''; g add -A; g commit -q -m bump
check "no changelog" "error: no CHANGELOG section for 0.0.2" 3

new_repo nolog-skip 0.0.1 ''
g tag v0.0.1
check "no changelog is fine when skipping" "skip 0.0.1" 0

# The real repository, read-only: whatever the answer, it is one line.
out=$("$script" 2>&1); code=$?
case $code in 0 | 3) ;; *) bad "real repo: exit $code" ;; esac
[ "$(printf '%s\n' "$out" | wc -l)" = 1 ] || bad "real repo: not exactly one line: $out"

[ "$fail" = 0 ] && echo "release-needed ok"
exit "$fail"
