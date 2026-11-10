#!/usr/bin/env bash
# run-shot.sh <out.png> <seconds> [command to run after start...]: start the spike, optionally run a
# command, screenshot the whole desktop, stop the spike. Prints the spike's stderr.
out=$1; secs=$2; shift 2
bin=${KYKO_SPIKE_BIN:-$(dirname "$0")/target/debug/kyko-spike}
log=$(mktemp)
"$bin" 2>"$log" & pid=$!
sleep "$secs"
[ $# -gt 0 ] && { "$@"; sleep 1; }
spectacle -b -n -f -o "$out" >/dev/null 2>&1
kill -0 $pid 2>/dev/null && echo "alive=yes" || echo "alive=no"
kill $pid 2>/dev/null; wait $pid 2>/dev/null
echo "--- stderr"; cat "$log"; rm -f "$log"
