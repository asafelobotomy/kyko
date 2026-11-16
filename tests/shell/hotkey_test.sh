#!/usr/bin/env bash
# The drop-down binds its toggle key through Hyprland's control socket (a fake one here), moves
# the binding when the config changes, and binds again when Hyprland reports a config reload.
# Skipped (exit 0) where sway, python3 or dbus-run-session is missing or the binary is not built.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in sway python3 dbus-run-session; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }

if [ -z "${KYKO_HOTKEY_INNER:-}" ]; then
    KYKO_HOTKEY_INNER=1 exec dbus-run-session -- "$0" "$@"
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
[ -n "$WAYLAND_DISPLAY" ] || { echo "skip: sway did not start"; exit 0; }

# A fake Hyprland: logs each control command, answers ok, and sends `configreloaded` on the
# event socket whenever a file named reload appears.
export HYPRLAND_INSTANCE_SIGNATURE=fake
hypr="$XDG_RUNTIME_DIR/hypr/fake"
mkdir -p "$hypr"
python3 - "$hypr" "$tmp" >"$tmp/fake.log" 2>&1 <<'PY' &
import os, socket, sys, threading, time
hypr, tmp = sys.argv[1:3]
def listen(name):
    s = socket.socket(socket.AF_UNIX); s.bind(os.path.join(hypr, name)); s.listen(5); return s
ctl, ev = listen(".socket.sock"), listen(".socket2.sock")
def serve():
    while True:
        c, _ = ctl.accept()
        data = c.recv(512).decode()
        with open(os.path.join(tmp, "binds.log"), "a") as f: f.write(data + "\n")
        c.sendall(b"ok"); c.close()
threading.Thread(target=serve, daemon=True).start()
conns = []
def accept():
    while True:
        c, _ = ev.accept(); conns.append(c)
threading.Thread(target=accept, daemon=True).start()
while True:
    if os.path.exists(os.path.join(tmp, "reload")):
        os.remove(os.path.join(tmp, "reload"))
        for c in conns:
            try: c.sendall(b"configreloaded>>\n")
            except OSError: pass
    time.sleep(0.1)
PY
pids+=($!)
for _ in $(seq 50); do [ -S "$hypr/.socket2.sock" ] && break; sleep 0.1; done
: > "$tmp/binds.log"

wait_for() { for _ in $(seq 40); do eval "$1" && return 0; sleep 0.25; done; return 1; }
binds() { grep -c "$1" "$tmp/binds.log"; }

printf '[theme]\nsource = "builtin:gruvbox-dark"\n' > "$XDG_CONFIG_HOME/kyko/config.toml"
"$bin" dropdown >>"$tmp/dd.log" 2>&1 & pids+=($!)

# 1. The default key, F12, is bound to `kyko toggle` at start.
wait_for 'grep -q "bind , *F12,exec,kyko toggle\|bind ,F12,exec,kyko toggle" "$tmp/binds.log"' \
    || bad "F12 was not bound ($(cat "$tmp/binds.log"))"

# 2. Changing the key in the config unbinds F12 and binds the new key.
printf '[hotkey]\ntoggle = "ctrl+alt+k"\n' >> "$XDG_CONFIG_HOME/kyko/config.toml"
wait_for 'grep -q "unbind" "$tmp/binds.log" && grep -q "bind CTRL ALT,k,exec,kyko toggle" "$tmp/binds.log"' \
    || bad "the key did not move ($(cat "$tmp/binds.log"))"

# 3. A reload of Hyprland's config forgets keywords; kyko binds again.
before=$(binds "bind CTRL ALT,k")
touch "$tmp/reload"
wait_for '[ "$(binds "bind CTRL ALT,k")" -gt "$before" ]' || bad "no rebind after a config reload"

[ $fail = 0 ] && echo "hotkey: ok"
exit $fail
