#!/usr/bin/env bash
# The shell integration scripts, run for real in a pty: each of bash, zsh and fish must emit the
# prompt-start, input-start, command-start, exit-status (0 and 1) and folder marks. A shell that is
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
            try: buf += os.read(fd, 65536)
            except OSError: return
rd(3)
for c in (b"true\r", b"false\r", b"exit\r"):
    os.write(fd, c); rd(1.5)
missing = [n for n, m in [("prompt start", b"\x1b]133;A\x07"), ("input start", b"\x1b]133;B\x07"),
           ("command start", b"\x1b]133;C\x07"), ("status 0", b"\x1b]133;D;0\x07"),
           ("status 1", b"\x1b]133;D;1\x07"), ("folder", b"\x1b]7;file://")] if m not in buf]
print(",".join(missing))
PY
)
    if [ -n "$out" ]; then echo "FAIL: $sh is missing marks: $out"; fail=1; else echo "$sh marks ok"; fi
done
exit $fail
