#!/usr/bin/env bash
# The hotkey backend on a real Hyprland (crates/kyko-hotkey/tests/live_hyprland.rs), run as root in
# a throwaway archlinux container given one DRM card (vkms, loaded on the runner):
#   docker run --rm --device "$card" -e SMOKE_CARD="$card" -e SMOKE_UID="$(id -u)" \
#       -v "$PWD:/src" archlinux:latest bash /src/scripts/hotkey-live.sh
# Hyprland's headless backend still allocates through a DRM node, and its DRM backend needs a seat,
# so this runs seatd for an unprivileged user (Hyprland refuses root). The Lua config holds uQ's
# F12 bind as uQ's generated.lua writes it.
set -eu
card=${SMOKE_CARD:?SMOKE_CARD must name the DRM card passed to the container}
pacman -Syu --noconfirm --needed rust gcc hyprland seatd wtype mesa jq >/dev/null
id tester >/dev/null 2>&1 || useradd -m -u "${SMOKE_UID:-1000}" tester
chmod 666 "$card"
SEATD_VTBOUND=0 seatd -u tester -l error &
su tester -c 'set -eu
    cd /src
    rt=$(mktemp -d); tmp=$(mktemp -d)
    cat > "$tmp/hypr.lua" <<LUA
hl.monitor({ output = "", mode = "1280x720", position = "auto", scale = 1 })
hl.config({ misc = { disable_hyprland_logo = true, disable_splash_rendering = true } })
hl.bind("F12", hl.dsp.exec_cmd("kyko toggle"), { description = "uq:uq.terminal.dropdown" })
hl.bind("SUPER + Return", hl.dsp.exec_cmd("kitty"), { description = "uq:uq.terminal" })
LUA
    env XDG_RUNTIME_DIR="$rt" LIBSEAT_BACKEND=seatd LIBGL_ALWAYS_SOFTWARE=1 Hyprland -c "$tmp/hypr.lua" >"$tmp/hypr.log" 2>&1 &
    hypr=$!
    sig=""
    for _ in $(seq 1 50); do
        kill -0 $hypr 2>/dev/null || { cat "$tmp/hypr.log"; exit 1; }
        sig=$(find "$rt/hypr" -mindepth 1 -maxdepth 1 -printf "%f\n" 2>/dev/null | head -n1)
        [ -n "$sig" ] && [ -S "$rt/hypr/$sig/.socket.sock" ] && break
        sleep 0.2
    done
    [ -n "$sig" ] || { echo "Hyprland never opened its socket"; tail -n 30 "$tmp/hypr.log"; exit 1; }
    wl=""
    for _ in $(seq 1 25); do
        wl=$(find "$rt" -maxdepth 1 -name "wayland-*" ! -name "*.lock" -printf "%f\n" | head -n1)
        [ -n "$wl" ] && break
        sleep 0.2
    done
    hc() { XDG_RUNTIME_DIR="$rt" HYPRLAND_INSTANCE_SIGNATURE="$sig" hyprctl "$@"; }
    hc version | head -n1
    [ -n "$(hc monitors -j | jq -r ".[0].name // empty")" ] || hc output create headless >/dev/null
    rc=0
    XDG_RUNTIME_DIR="$rt" HYPRLAND_INSTANCE_SIGNATURE="$sig" WAYLAND_DISPLAY="$wl" \
        KYKO_LIVE_HYPRLAND=1 KYKO_LIVE_MARKER="$tmp/fired" \
        cargo test --locked -p kyko-hotkey --test live_hyprland -- --nocapture || rc=$?
    [ $rc = 0 ] || { echo "--- hypr.log"; tail -n 40 "$rt/hypr/$sig/hyprland.log" 2>/dev/null || tail -n 40 "$tmp/hypr.log"; }
    kill $hypr 2>/dev/null || true
    exit $rc'
