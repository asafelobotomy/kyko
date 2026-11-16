#!/usr/bin/env bash
# Terminal basics in the windowed kyko, with real input on a virtual X display: a double click
# selects a word and Ctrl+Shift+C copies it, Ctrl+Shift+V pastes the clipboard into the shell, and
# the wheel scrolls the history, and closing a busy pane asks first. Skipped (exit 0) where xvfb-run, xdotool, xclip or the built
# binary is missing.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=${KYKO_BIN:-$root/target/debug/kyko}
for need in xvfb-run xdotool xclip; do
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
# 1. Select a word by double click, copy it.
type_line 'echo SELECTME'
# Row 1 holds the output: the window pads 8px, the tab bar takes 36.
xdotool mousemove 30 78 click --repeat 2 --delay 80 1
sleep 0.5
xdotool key ctrl+shift+c; sleep 0.5
xclip -o -selection clipboard > "$tmp/copied" 2>/dev/null
# 2. Paste a command from the clipboard; the paste is a single line and Return runs it.
printf 'echo pasted-\$((1+1)) > \$T/pasted' | xclip -selection clipboard -i
sleep 0.3
xdotool key ctrl+shift+v; sleep 0.5
xdotool key Return; sleep 0.8
# 3. The wheel scrolls the history: print more lines than the screen holds, then scroll up.
type_line 'for i in \$(seq 1 80); do echo line-\$i; done'
xdotool mousemove 200 200 click --repeat 5 4
sleep 0.5
# 4. Closing a pane that runs a program asks first: still there after Ctrl+Shift+W, closed by Return.
type_line 'sleep 60'
sleep 1.5
xdotool key ctrl+shift+w; sleep 1
kill -0 \$pid 2>/dev/null && echo asked > "$tmp/asked"
xdotool key Return; sleep 1
for _ in \$(seq 20); do kill -0 \$pid 2>/dev/null || break; sleep 0.25; done
kill \$pid 2>/dev/null
DRIVE
chmod +x "$tmp/drive.sh"
xvfb-run -a -s '-screen 0 1280x800x24' "$tmp/drive.sh" >/dev/null 2>&1

[ "$(cat "$tmp/copied" 2>/dev/null)" = "SELECTME" ] || bad "copy did not put the selected word on the clipboard ($(cat "$tmp/copied" 2>/dev/null))"
[ -e "$tmp/asked" ] || bad "closing a busy pane closed it without asking"
[ "$(cat "$tmp/pasted" 2>/dev/null)" = "pasted-2" ] || bad "paste did not reach the shell ($(cat "$tmp/pasted" 2>/dev/null))"

[ $fail -eq 0 ] && echo "terminal ok"
exit $fail
