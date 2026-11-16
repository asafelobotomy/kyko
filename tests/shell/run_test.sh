#!/usr/bin/env bash
# `kyko run` and `kyko new-tab` against the drop-down on a headless sway: the command runs in
# the folder given, bad commands are refused, `advanced.allow_run = false` turns it off, and the
# window is shown. Skipped (exit 0) where sway, grim or dbus-run-session is missing or the binary
# is not built.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in sway grim dbus-run-session; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }

if [ -z "${KYKO_RUN_INNER:-}" ]; then
    # Run the checks inside a private session bus.
    KYKO_RUN_INNER=1 exec dbus-run-session -- "$0" "$@"
fi

fail=0
bad() { echo "FAIL: $*"; fail=1; }
tmp=$(mktemp -d)
export XDG_RUNTIME_DIR="$tmp/run" XDG_CONFIG_HOME="$tmp/config" XDG_STATE_HOME="$tmp/state"
mkdir -p "$XDG_RUNTIME_DIR" "$XDG_CONFIG_HOME/kyko" "$XDG_STATE_HOME"
chmod 700 "$XDG_RUNTIME_DIR"
export WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman
export QT_QPA_PLATFORM=wayland LIBGL_ALWAYS_SOFTWARE=1 QT_FORCE_STDERR_LOGGING=1
pids=()
cleanup() {
    for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
    pkill -f "kykod --socket $XDG_RUNTIME_DIR" 2>/dev/null
    rm -rf "$tmp"
}
trap cleanup EXIT

echo "output HEADLESS-1 mode 1280x800" > "$tmp/sway.conf"
sway -c "$tmp/sway.conf" >"$tmp/sway.log" 2>&1 & pids+=($!)
for _ in $(seq 50); do ls "$XDG_RUNTIME_DIR" | grep -qE '^wayland-[0-9]+$' && break; sleep 0.1; done
export WAYLAND_DISPLAY=$(ls "$XDG_RUNTIME_DIR" | grep -E '^wayland-[0-9]+$' | head -1)
[ -n "$WAYLAND_DISPLAY" ] || { echo "skip: sway did not start ($(tail -2 "$tmp/sway.log"))"; exit 0; }

# pixel X Y -> rrggbb of the screen
shot() { grim -t ppm "$tmp/shot.ppm"; }
pixel() {
    local hdr; hdr=$(head -c 20 "$tmp/shot.ppm" | head -3 | wc -c)
    od -An -tx1 -v -j $(( hdr + ($2 * 1280 + $1) * 3 )) -N3 "$tmp/shot.ppm" | tr -d ' \n'
}
visible() { busctl --user get-property org.kyko.Terminal1 /org/kyko/Terminal1 org.kyko.Terminal1 Visible 2>/dev/null | awk '{print $2}'; }
wait_for() { for _ in $(seq 40); do eval "$1" && return 0; sleep 0.25; done; return 1; }

start() { "$bin" dropdown >>"$tmp/dd.log" 2>&1 & dd=$!; pids+=($dd); wait_for '[ -n "$(visible)" ]' || bad "the drop-down never took its bus name ($(tail -3 "$tmp/dd.log"))"; }

printf '[theme]\nsource = "builtin:gruvbox-dark"\n' > "$XDG_CONFIG_HOME/kyko/config.toml"
start
mkdir -p "$tmp/work"

# 1. A command runs through the shell in the given folder, and the window is shown.
"$bin" run -d "$tmp/work" 'pwd > ran.txt; echo "$((6*7))" >> ran.txt' || bad "kyko run failed"
wait_for '[ -s "$tmp/work/ran.txt" ] && [ "$(wc -l < "$tmp/work/ran.txt")" = 2 ]' || bad "the command did not run in its folder"
[ "$(sed -n 1p "$tmp/work/ran.txt")" = "$tmp/work" ] || bad "wrong folder: $(sed -n 1p "$tmp/work/ran.txt")"
[ "$(sed -n 2p "$tmp/work/ran.txt")" = 42 ] || bad "the shell did not evaluate the command"
wait_for '[ "$(visible)" = true ]' || bad "run did not show the window"
sleep 1.2
# KYKO_SHOT=file.png saves a screenshot of the held tab, for reports.
if [ -n "${KYKO_SHOT:-}" ]; then grim "$KYKO_SHOT"; fi

# 2. Bad commands are refused before any tab opens.
"$bin" run '' 2>"$tmp/err"; [ $? = 2 ] || bad "an empty command was not refused (exit $?)"
long=$(head -c 5000 /dev/zero | tr '\0' a)
"$bin" run "$long" 2>"$tmp/err"; [ $? = 2 ] || bad "a long command was not refused"

# 3. new-tab opens a tab in the folder given.
"$bin" new-tab -d "$tmp/work" || bad "kyko new-tab failed"

# 4. advanced.allow_run = false turns Run off; the window says why.
printf '[theme]\nsource = "builtin:gruvbox-dark"\n[advanced]\nallow_run = false\n' > "$XDG_CONFIG_HOME/kyko/config.toml"
sleep 1.5
"$bin" run 'touch "$HOME/nope"' 2>"$tmp/err"; rc=$?
[ $rc = 1 ] && grep -q allow_run "$tmp/err" || bad "allow_run = false did not refuse (exit $rc: $(cat "$tmp/err"))"

[ $fail = 0 ] && echo "run ok"
exit $fail
