#!/usr/bin/env bash
# The prompt marks in the kyko window, under Xvfb with a real bash: the gutter bar is green after a
# command that succeeded and red after one that failed, Ctrl+Shift+O copies the last command's
# output, and Ctrl+Shift+Up scrolls back to an earlier prompt.
# Skipped (exit 0) where Xvfb, xdotool, xclip or ImageMagick is missing or the binaries are not built.
# KYKO_SHOT_DIR=<dir> keeps screenshots of the steps there.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in Xvfb xdotool xclip import bash; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }
[ -x "$(dirname "$bin")/kyko-prompt" ] || echo "note: no kyko-prompt next to $bin; the plain prompt is used"

fail=0
bad() { echo "FAIL: $*"; fail=1; }
tmp=$(mktemp -d)
export HOME="$tmp" XDG_CONFIG_HOME="$tmp/config" XDG_STATE_HOME="$tmp/state" XDG_RUNTIME_DIR="$tmp/run"
mkdir -p "$XDG_CONFIG_HOME/kyko" "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
export LIBGL_ALWAYS_SOFTWARE=1 QT_QPA_PLATFORM=xcb SHELL=/bin/bash
export PATH="$(dirname "$bin"):$PATH"
printf '[theme]\nsource = "builtin:gruvbox-dark"\n[window]\nshape = "sheet"\n' > "$XDG_CONFIG_HOME/kyko/config.toml"
pids=()
cleanup() {
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
    pkill -f "kykod --socket $XDG_RUNTIME_DIR" 2>/dev/null
    rm -rf "$tmp"
}
trap cleanup EXIT

disp=:$((70 + $$ % 20))
Xvfb "$disp" -screen 0 1000x700x24 >/dev/null 2>&1 & pids+=($!)
export DISPLAY=$disp
sleep 1
wait_for() { for _ in $(seq 40); do eval "$1" && return 0; sleep 0.25; done; return 1; }
shot() { import -window root "$tmp/$1.png"; [ -z "${KYKO_SHOT_DIR:-}" ] || cp "$tmp/$1.png" "$KYKO_SHOT_DIR/$1.png"; }
# How many pixels in the gutter column (x = 9: the window's 8 px padding, then the bar) have this colour.
bar() { import -window root -depth 8 -crop 1x600+9+40 txt:- 2>/dev/null | grep -ci "$1"; }
GREEN='#B8BB26' RED='#FB4934'
run() { xdotool type --delay 25 -- "$1"; xdotool key Return; sleep "${2:-0.9}"; }

"$bin" >"$tmp/log" 2>&1 & pids+=($!)
wait_for 'xdotool search --name kyko >/dev/null 2>&1' || bad "no kyko window ($(tail -2 "$tmp/log"))"
sleep 3

# 1. The bar follows the exit status.
run 'echo fine'
run 'false'
shot marks-gutter
[ "$(bar "$GREEN")" -ge 2 ] || bad "no green bar after a command that succeeded"
[ "$(bar "$RED")" -ge 2 ] || bad "no red bar after a command that failed"

# 2. Ctrl+Shift+O copies what the last command printed, and not the prompt.
run "printf 'alpha\\nbeta\\n'"
xdotool key ctrl+shift+o; sleep 0.8
got=$(xclip -selection clipboard -o 2>/dev/null)
[ "$got" = "$(printf 'alpha\nbeta')" ] || bad "copied the wrong output: $(printf '%s' "$got" | head -c 80 | od -c | head -3)"
shot marks-copy

# 3. Ctrl+Shift+Up scrolls back to an earlier prompt.
run 'seq 1 80' 1.5
shot marks-bottom
cp "$tmp/marks-bottom.png" "$tmp/before.png"
xdotool key ctrl+shift+Up; sleep 0.8
shot marks-jumped
[ "$(import -window root -depth 8 txt:- | md5sum)" != "$(convert "$tmp/before.png" -depth 8 txt:- | md5sum)" ] || bad "the view did not move"
xdotool key ctrl+shift+Up; sleep 0.5
xdotool key ctrl+shift+Down; sleep 0.5
shot marks-back

[ $fail = 0 ] && echo "marks ok"
exit $fail
