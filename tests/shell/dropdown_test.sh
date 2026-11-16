#!/usr/bin/env bash
# The drop-down on a headless sway: it slides in over a layer-shell surface on `kyko toggle`,
# hides on the next one, follows a config change without a restart, and keeps its shell when the
# window is stopped and started again. Pixels are read from a grim screenshot.
# Skipped (exit 0) where sway, grim or dbus-run-session is missing or the binary is not built.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in sway grim dbus-run-session; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }

if [ -z "${KYKO_DROPDOWN_INNER:-}" ]; then
    # Run the checks inside a private session bus.
    KYKO_DROPDOWN_INNER=1 exec dbus-run-session -- "$0" "$@"
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
shot; desk=$(pixel 10 700)

# 1. Hidden at first; toggle shows it, toggle hides it.
[ "$(visible)" = false ] || bad "visible at start"
"$bin" toggle || bad "kyko toggle failed"
wait_for '[ "$(visible)" = true ]' || bad "toggle did not show it"
sleep 1.2; shot
dark=$(pixel 640 200)
[ "$dark" != "$desk" ] || bad "nothing is drawn at the top of the screen after toggle ($dark)"
[ "$(pixel 640 700)" = "$desk" ] || bad "the drop-down covers more than its panel"
"$bin" toggle
wait_for '[ "$(visible)" = false ]' || bad "second toggle did not hide it"
sleep 1; shot
[ "$(pixel 640 200)" = "$desk" ] || bad "the panel is still on screen after hiding"

# 2. show and hide are idempotent.
"$bin" show; "$bin" show; wait_for '[ "$(visible)" = true ]' || bad "show"
"$bin" hide; "$bin" hide; wait_for '[ "$(visible)" = false ]' || bad "hide"

# 3. A config change recolours the open window without a restart.
"$bin" show; wait_for '[ "$(visible)" = true ]'; sleep 1.2
printf '[theme]\nsource = "builtin:gruvbox-light"\n' > "$XDG_CONFIG_HOME/kyko/config.toml"
changed() { shot; [ "$(pixel 640 200)" != "$dark" ]; }
wait_for changed || bad "the panel kept its colours after the config changed"

# 4. The shell outlives the window: the same shell answers after a restart.
holder=$(pgrep -f "kykod --socket $XDG_RUNTIME_DIR" | head -1)
[ -n "$holder" ] || bad "no shared kykod is running"
before=$(pgrep -P "$holder" | sort | tr '\n' ' ')
[ -n "$before" ] || bad "kykod holds no shell"
kill "$dd"; wait "$dd" 2>/dev/null
start
after=$(pgrep -P "$holder" | sort | tr '\n' ' ')
[ "$before" = "$after" ] || bad "the shell changed across a window restart ($before -> $after)"

# 5. A second drop-down does not start.
"$bin" dropdown >/dev/null 2>&1 && bad "a second drop-down started"

[ $fail -eq 0 ] && echo "dropdown ok"
exit $fail
