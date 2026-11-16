#!/usr/bin/env bash
# The window against a real kykod, on a virtual X display: it runs a command, draws the screen,
# takes real key presses, and closes with the shell's exit status. Skipped (exit 0) where
# xvfb-run, xdotool or the built binary is missing.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in xvfb-run xdotool; do
    command -v "$need" >/dev/null 2>&1 || { echo "skip: $need missing"; exit 0; }
done
[ -x "$bin" ] || { echo "skip: $bin not built"; exit 0; }
fail=0
bad() { echo "FAIL: $*"; fail=1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export QT_QPA_PLATFORM=xcb XDG_RUNTIME_DIR="$tmp/run"
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"

# 1. The exit status of the program comes out as the window's.
xvfb-run -a "$bin" -e sh -c 'exit 7' >/dev/null 2>&1
[ $? -eq 7 ] || bad "kyko -e 'exit 7' did not exit 7"

# 2. Typed keys reach the shell and what it prints is drawn: the window saves a picture of its
#    screen, and a screen with text on it is far larger than the blank one (about 2 KB).
drive() { # drive <seconds before the keys> <keys to type> <extra env...>
    cat > "$tmp/drive.sh" <<DRIVE
#!/usr/bin/env bash
export KYKO_TEST_SHOT="$tmp/shot.png" KYKO_TEST_MS=5000
"$bin" -e sh 2>"$tmp/err" &
pid=\$!
sleep 2.5
xdotool type --delay 30 '$1'
xdotool key Return
wait \$pid
echo "status=\$?" > "$tmp/status"
DRIVE
    chmod +x "$tmp/drive.sh"
    rm -f "$tmp/shot.png" "$tmp/status"
    xvfb-run -a -s '-screen 0 1280x800x24' "$tmp/drive.sh" >/dev/null 2>&1
}
drive 'echo typed-$((6*7)); echo more-text-on-the-screen; ls /'
[ -s "$tmp/shot.png" ] || bad "no screenshot saved ($(tail -2 "$tmp/err" 2>/dev/null))"
size=$(wc -c < "$tmp/shot.png" 2>/dev/null || echo 0)
[ "$size" -gt 8000 ] || bad "the screen looks blank (picture of $size bytes)"

# 3. A typed `exit 5` ends the window with the shell's status.
drive 'exit 5'
grep -q 'status=5' "$tmp/status" 2>/dev/null || bad "typed keys did not reach the shell ($(cat "$tmp/status" 2>/dev/null))"

[ $fail -eq 0 ] && echo "window smoke ok"
exit $fail
