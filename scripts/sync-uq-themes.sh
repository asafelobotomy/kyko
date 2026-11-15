#!/usr/bin/env bash
# Copies uQ's themes into kyko as uQ itself renders them:
#
#   scripts/sync-uq-themes.sh <uq checkout>
#
# Runs `uqctl render-themes` in the uQ checkout, against that checkout's own themes and
# templates, and copies each <id>-<variant>/tokens.json to crates/kyko-core/themes/
# <id>-<variant>.json. Writes crates/kyko-core/themes/CREDITS.md from uQ's themes/CREDITS.md.
# The files are committed and compiled into kyko-core, so building kyko needs neither uQ nor
# this script; re-running it when uQ's themes change is a deliberate commit.
set -euo pipefail

if [[ $# -ne 1 || ! -d $1/themes || ! -d $1/templates ]]; then
    echo "usage: $0 <uq checkout>" >&2
    exit 2
fi
uq=$(cd "$1" && pwd)
here=$(cd "$(dirname "$0")/.." && pwd)
dest=$here/crates/kyko-core/themes
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

(cd "$uq" && cargo run -q -p uqctl -- render-themes \
    --themes "$uq/themes" --templates "$uq/templates" "$tmp/out")

mkdir -p "$dest"
rm -f "$dest"/*.json
count=0
for dir in "$tmp"/out/*/; do
    key=$(basename "$dir")
    cp "$dir/tokens.json" "$dest/$key.json"
    count=$((count + 1))
done
if [[ $count -eq 0 ]]; then
    echo "uqctl rendered no themes" >&2
    exit 1
fi

copyright=$(grep -m1 '^Copyright' "$uq/LICENSE")
{
    echo "# Theme credits"
    echo
    echo "kyko's built-in themes are uQ's (https://github.com/asafelobotomy/uq), rendered by uQ"
    echo "and copied by \`scripts/sync-uq-themes.sh\`. uQ is under the MIT licence:"
    echo "$copyright."
    echo
    # uQ's own credits, below its title, with its references to itself pointed at uQ.
    tail -n +3 "$uq/themes/CREDITS.md" |
        sed -e 's|(this repository)|(uQ, https://github.com/asafelobotomy/uq)|g' \
            -e 's|see `crates/uq-core/src/theme.rs`|see uQ'"'"'s `crates/uq-core/src/theme.rs`|g'
} > "$dest/CREDITS.md"

echo "copied $count themes into ${dest#"$here"/}"
