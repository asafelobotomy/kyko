#!/usr/bin/env bash
# Tabs and splits in the windowed kyko, driven by real key presses on a virtual X display: each
# shell writes its own pid to a file, so which shell took which keys can be checked. Skipped
# (exit 0) where xvfb-run, xdotool or the built binary is missing.
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
export QT_QPA_PLATFORM=xcb XDG_RUNTIME_DIR="$tmp/run" SHELL=/bin/sh T="$tmp"
mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"

cat > "$tmp/drive.sh" <<DRIVE
#!/usr/bin/env bash
type_line() { xdotool type --delay 20 "\$1"; xdotool key Return; sleep 0.6; }
"$bin" -e sh 2>"$tmp/err" &
pid=\$!
sleep 3
type_line 'echo \$\$ > \$T/p1'
xdotool key ctrl+shift+t; sleep 1
type_line 'echo \$\$ > \$T/p2'
xdotool key alt+1; sleep 0.5
type_line 'echo \$\$ > \$T/p3'
xdotool key ctrl+shift+d; sleep 1
type_line 'echo \$\$ > \$T/p4'
xdotool key alt+Left; sleep 0.5
type_line 'echo \$\$ > \$T/p5'
xdotool key alt+2; sleep 0.5
type_line 'echo \$\$ > \$T/p6'
# Close everything: the pane in tab 2, then tab 1's two panes.
xdotool key ctrl+shift+w; sleep 0.5
xdotool key ctrl+shift+w; sleep 0.5
xdotool key ctrl+shift+w; sleep 0.5
for _ in \$(seq 40); do kill -0 \$pid 2>/dev/null || break; sleep 0.25; done
if kill -0 \$pid 2>/dev/null; then echo alive > "$tmp/alive"; kill \$pid; fi
DRIVE
chmod +x "$tmp/drive.sh"
xvfb-run -a -s '-screen 0 1280x800x24' "$tmp/drive.sh" >/dev/null 2>&1

p() { cat "$tmp/p$1" 2>/dev/null; }
for n in 1 2 3 4 5 6; do [ -s "$tmp/p$n" ] || bad "p$n was not written (keys did not reach a shell)"; done
[ -n "$(p 1)" ] && [ "$(p 1)" = "$(p 3)" ] || bad "Alt+1 did not return to the first tab's shell ($(p 1) vs $(p 3))"
[ "$(p 2)" != "$(p 1)" ] || bad "Ctrl+Shift+T did not open a second shell"
[ "$(p 4)" != "$(p 1)" ] && [ "$(p 4)" != "$(p 2)" ] || bad "Ctrl+Shift+D did not open a third shell"
[ "$(p 5)" = "$(p 1)" ] || bad "Alt+Left did not focus the left pane ($(p 5) vs $(p 1))"
[ "$(p 6)" = "$(p 2)" ] || bad "Alt+2 did not select the second tab ($(p 6) vs $(p 2))"
[ ! -e "$tmp/alive" ] || bad "closing the last pane did not end the window"

[ $fail -eq 0 ] && echo "tabs ok"
exit $fail
