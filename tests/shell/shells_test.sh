#!/usr/bin/env bash
# The shell integration scripts, run for real in a pty: each of bash, zsh and fish must emit the
# prompt-start, input-start, command-start (with the command line), exit-status (0 and 1) and
# folder marks, each once. A shell that is
# not installed, or no python3, is skipped (exit 0).
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
command -v python3 >/dev/null 2>&1 || { echo "skip: python3 missing"; exit 0; }
fail=0
for sh in bash zsh fish; do
    command -v "$sh" >/dev/null 2>&1 || { echo "skip: $sh missing"; continue; }
    out=$(python3 - "$sh" "$root" <<'PY'
import os, pty, select, shutil, sys, tempfile, time
shell, root = sys.argv[1], sys.argv[2]
d = tempfile.mkdtemp()
home = d + "/home"; sc = d + "/shell"
os.makedirs(home); os.makedirs(sc + "/zsh")
for n in ["bash.sh", "fish.fish", "zsh/.zshenv", "zsh/.zshrc"]:
    shutil.copy(f"{root}/crates/kyko-core/shell/{n}", f"{sc}/{n}")
env = dict(os.environ, HOME=home, TERM="xterm-256color", KYKO_PROMPT="1", KYKO_EXTRAS="0")
env["PATH"] = f"{root}/target/debug:" + os.environ["PATH"]
args = [shell]
if shell == "bash": args += ["--rcfile", sc + "/bash.sh", "-i"]
if shell == "zsh": env["ZDOTDIR"] = sc + "/zsh"; args += ["-i"]
if shell == "fish": args += ["--init-command", f"source '{sc}/fish.fish'", "-i"]
pid, fd = pty.fork()
if pid == 0:
    os.chdir(home); os.execvpe(shell, args, env)
buf = b""
def rd(t):
    global buf
    end = time.time() + t
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.2)
        if r:
            try: data = os.read(fd, 65536)
            except OSError: return
            buf += data
            # fish 4 asks the terminal who it is (primary device attributes) and waits.
            if b"\x1b[c" in data or b"\x1b[0c" in data:
                os.write(fd, b"\x1b[?62;c")
rd(4)
for c in (b"true\r", b"false\r", b"exit\r"):
    os.write(fd, c); rd(1.5)
# A mark may carry parameters and end with BEL or ST (fish 4 marks on its own, with ST). Each
# shell sends the command line with the command start.
import re
end = rb"(;[^\x07\x1b]*)?(\x07|\x1b\\)"
missing = [n for n, m in [("prompt start", rb"\x1b\]133;A" + end), ("input start", rb"\x1b\]133;B" + end),
           ("command start", rb"\x1b\]133;C;cmdline_url=false(\x07|\x1b\\)"),
           ("status 0", rb"\x1b\]133;D;0" + end), ("status 1", rb"\x1b\]133;D;1" + end),
           ("folder", rb"\x1b\]7;file://")] if not re.search(m, buf)]
# Each mark once: fish 4's own marks and kyko's are not both sent.
twice = [k for k in (b"A", b"C", b"D") if len(re.findall(rb"\x1b\]133;" + k, buf)) > 4]
missing += [f"{k.decode()} sent twice" for k in twice]
print(",".join(missing))
if missing: print("output: " + repr(buf[-600:]), file=sys.stderr)
PY
)
    if [ -n "$out" ]; then echo "FAIL: $sh is missing marks: $out"; fail=1; else echo "$sh marks ok"; fi
done
exit $fail
