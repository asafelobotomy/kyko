#!/usr/bin/env bash
# kyko as a uQ launcher provider and `ListTabs` / `FocusTab`, against the drop-down on a headless
# sway: Search finds a tab by name, Activate focuses it and refuses ids it never returned.
# Skipped (exit 0) where sway, grim, busctl or dbus-run-session is missing or the binary is not built.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in sway grim busctl dbus-run-session; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }

if [ -z "${KYKO_TABS_INNER:-}" ]; then
    # Run the checks inside a private session bus.
    KYKO_TABS_INNER=1 exec dbus-run-session -- "$0" "$@"
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
mkdir -p "$tmp/xyzzy-folder"
"$bin" new-tab -d "$tmp/xyzzy-folder" || bad "new-tab failed"
P="busctl --user call org.kyko.Terminal1 /org/uq/Launcher1 org.uq.Launcher1.Provider"
T="busctl --user call org.kyko.Terminal1 /org/kyko/Terminal1 org.kyko.Terminal1"

# 1. Search finds the tab by its name; the row has the shape uQ reads.
wait_for '$P Search s xyzzy 2>/dev/null | grep -q xyzzy' || bad "Search did not find the tab ($($P Search s xyzzy 2>&1))"
out=$($P Search s xyzzy)
case "$out" in "a(sssssd) 1 "*) ;; *) bad "unexpected shape: $out";; esac
id=$(echo "$out" | sed -n 's/^a(sssssd) 1 "\([0-9]*\)".*/\1/p')
[ -n "$id" ] || bad "no tab id in: $out"
[ "$($P Search s nothing-matches-this | awk '{print $2}')" = 0 ] || bad "a miss returned rows"

# 2. ListTabs has both tabs; Activate focuses and shows the window.
$T ListTabs | grep -q xyzzy || bad "ListTabs lacks the tab"
"$bin" hide; wait_for '[ "$(visible)" = false ]' || bad "hide failed"
$P Activate ss "$id" "" || bad "Activate failed"
wait_for '[ "$(visible)" = true ]' || bad "Activate did not show the window"

# 3. Ids kyko never returned are refused.
$P Activate ss 999 "" 2>/dev/null && bad "Activate took an unknown tab"
$P Activate ss "rm -rf" "" 2>/dev/null && bad "Activate took a non-id"
$T FocusTab u 999 2>/dev/null && bad "FocusTab took an unknown tab"

[ $fail = 0 ] && echo "tabs provider ok"
exit $fail
