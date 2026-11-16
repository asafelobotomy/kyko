#!/usr/bin/env bash
# The Preferences window under Xvfb: a click on a switch is written to config.toml, a hand-made
# comment survives it, and "Reset this page" takes the page's keys out again. The positions are
# those of the 760x520 window with no window manager.
# Skipped (exit 0) where Xvfb or xdotool is missing or the binary is not built.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in Xvfb xdotool; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }

fail=0
bad() { echo "FAIL: $*"; fail=1; }
tmp=$(mktemp -d)
export XDG_CONFIG_HOME="$tmp/config" HOME="$tmp" XDG_RUNTIME_DIR="$tmp/run"
mkdir -p "$XDG_CONFIG_HOME/kyko" "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
export LIBGL_ALWAYS_SOFTWARE=1 QT_QPA_PLATFORM=xcb
cfg="$XDG_CONFIG_HOME/kyko/config.toml"
pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done; rm -rf "$tmp"; }
trap cleanup EXIT

disp=:$((60 + $$ % 30))
Xvfb "$disp" -screen 0 1000x700x24 >/dev/null 2>&1 & pids+=($!)
export DISPLAY=$disp
sleep 1
wait_for() { for _ in $(seq 40); do eval "$1" && return 0; sleep 0.25; done; return 1; }
click() { xdotool mousemove "$1" "$2" click 1; }

printf '# mine\n[hotkey]\ntoggle = "F11"\n' > "$cfg"
"$bin" preferences >"$tmp/log" 2>&1 & pids+=($!)
wait_for 'xdotool search --name "kyko Preferences" >/dev/null 2>&1' || bad "no Preferences window ($(tail -2 "$tmp/log"))"
sleep 1.5

# 1. The "Slide in and out" switch writes window.animations = false; the rest of the file stays.
click 690 376
wait_for 'grep -q "animations = false" "$cfg"' || bad "the switch was not written ($(cat "$cfg"))"
grep -q '^# mine' "$cfg" && grep -q 'toggle = "F11"' "$cfg" || bad "the file's other content changed: $(cat "$cfg")"

# 2. Another page: Copy on select.
click 60 60
sleep 0.5
click 690 264
wait_for 'grep -q "copy_on_select = true" "$cfg"' || bad "copy_on_select was not written ($(cat "$cfg"))"

# 3. Reset this page (Terminal) removes its keys only.
click 688 480
wait_for '! grep -q "copy_on_select" "$cfg"' || bad "reset did not remove copy_on_select"
grep -q "animations = false" "$cfg" || bad "reset touched another page"

# The window's own shot, for reports.
if [ -n "${KYKO_SHOT:-}" ]; then
    command -v import >/dev/null && import -window root "$KYKO_SHOT"
fi
[ $fail = 0 ] && echo "prefs ok"
exit $fail
