#!/usr/bin/env bash
# The kyko command line, which needs no display: --version, --help and a bad flag. (These were
# an integration test in apps/kyko until that crate linked Qt: cxx-qt's initialisers are only
# linked into the binary, so a test binary of the crate cannot link.)
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }
fail=0
bad() { echo "FAIL: $*"; fail=1; }
want=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -1)

[ "$("$bin" --version 2>&1)" = "kyko $want" ] || bad "--version is not 'kyko $want'"
help=$("$bin" --help 2>&1)
for flag in --exec --title --class --working-directory; do
    case $help in *"$flag"*) ;; *) bad "$flag missing from --help";; esac
done
"$bin" --nope >/dev/null 2>&1
[ $? -eq 2 ] || bad "an unknown flag did not exit 2"

[ $fail -eq 0 ] && echo "cli ok"
exit $fail
