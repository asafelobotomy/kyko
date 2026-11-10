#!/usr/bin/env bash
# Re-run the spike's measurements. Everything is injected, so nothing needs typing and the spike
# never takes the keyboard. Results go to stdout; compare with spike/NOTES.md.
set -u
here=$(cd "$(dirname "$0")" && pwd); bin=$here/target/release/kyko-spike
[ -x "$bin" ] || { echo "cargo build --release --manifest-path $here/Cargo.toml first"; exit 1; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
export QT_FORCE_STDERR_LOGGING=1 KYKO_SPIKE_NOFOCUS=1 KYKO_SPIKE_SHELL=/bin/bash KYKO_SPIKE_NO_PORTAL=1
python3 - > "$tmp/dense.txt" <<'PY'
import random
random.seed(1)
chars="abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789{}[]()<>=+-*/_.,;:"
for r in range(70):
    line=""; c=0
    while c<230:
        n=random.randint(2,7); fg=random.randint(0,255); bg=random.choice([None,None,random.randint(0,255)])
        line+=f"\x1b[38;5;{fg}m"+(f"\x1b[48;5;{bg}m" if bg is not None else "\x1b[49m")
        line+="".join(random.choice(chars) for _ in range(n)); c+=n
    print(line+"\x1b[0m")
PY
run() { # run <seconds> [VAR=value...]: start the spike with those variables, stop it, print its stderr
    local secs=$1; shift
    env "$@" "$bin" 2>"$tmp/err" & local pid=$!
    sleep "$secs"; kill $pid 2>/dev/null; wait $pid 2>/dev/null; cat "$tmp/err"
}
echo "load: $(cut -d' ' -f1-3 /proc/loadavg)"
echo "== full repaint every frame, dense colour screen, full-size grid, row cache on"
run 7 KYKO_SPIKE_STATS=1 KYKO_SPIKE_BIG=1 KYKO_SPIKE_FULL=1 KYKO_SPIKE_TYPE="clear; cat $tmp/dense.txt\\n" | grep paint_ms | tail -1
echo "== the same with the row cache off (every row redrawn from text)"
run 12 KYKO_SPIKE_STATS=1 KYKO_SPIKE_BIG=1 KYKO_SPIKE_FULL=1 KYKO_SPIKE_NOCACHE=1 KYKO_SPIKE_TYPE="clear; cat $tmp/dense.txt\\n" | grep paint_ms | tail -1
echo "== plain text screen, row cache off"
run 7 KYKO_SPIKE_STATS=1 KYKO_SPIKE_BIG=1 KYKO_SPIKE_FULL=1 KYKO_SPIKE_NOCACHE=1 KYKO_SPIKE_TYPE="ls -la /usr/bin\\n" | grep paint_ms | tail -1
echo "== flood: time seq 1 100000"
run 6 KYKO_SPIKE_BIG=1 KYKO_SPIKE_TYPE="{ time seq 1 100000 ; } 2> $tmp/seq.time\\n" >/dev/null; grep real "$tmp/seq.time"
echo "== Ctrl+C during 'yes': see spike/NOTES.md for the probe (needs pgrep polling)"
