#!/usr/bin/env bash
# Try the spike by hand: runs it in its own app scope with a temporary desktop entry (the portal needs
# both to bind a hotkey), then removes the entry and the shortcut again on exit.
#   spike/try.sh                 # hotkey Meta+F12, panel shown, takes the keyboard
#   KEY='CTRL+F12' spike/try.sh  # ask for another key (only honoured the first time; see NOTES.md)
set -u
here=$(cd "$(dirname "$0")" && pwd)
bin=$here/target/release/kyko-spike
[ -x "$bin" ] || bin=$here/target/debug/kyko-spike
[ -x "$bin" ] || { echo "build first: cargo build --release --manifest-path $here/Cargo.toml"; exit 1; }
entry=$HOME/.local/share/applications/org.kyko.spike.desktop
cleanup() {
    qdbus6 org.kde.kglobalaccel /kglobalaccel org.kde.KGlobalAccel.unregister org.kyko.spike toggle >/dev/null 2>&1
    rm -f "$entry"
}
trap cleanup EXIT
printf '[Desktop Entry]\nType=Application\nName=kyko spike\nExec=%s\nNoDisplay=true\n' "$bin" > "$entry"
systemd-run --user --scope --quiet --unit="app-org.kyko.spike-$RANDOM.scope" \
    env QT_FORCE_STDERR_LOGGING=1 KYKO_SPIKE_HOTKEY="${KEY:-LOGO+F12}" "$@" "$bin"
