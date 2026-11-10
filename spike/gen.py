#!/usr/bin/env python3
"""Fill the terminal completely. gen.py dense|plain [frames]: every cell printable, no blanks.
dense changes colour every 2-7 cells; plain is one colour. With frames > 1, redraws that many
different full screens in place (a full-screen change each time)."""
import os, random, sys, time
mode = sys.argv[1] if len(sys.argv) > 1 else "dense"
frames = int(sys.argv[2]) if len(sys.argv) > 2 else 1
cols, rows = os.get_terminal_size()
chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789{}[]()<>=+-*/_.,;:#@%&"
random.seed(1)
def screen():
    out = ["\x1b[H"]
    for r in range(rows):
        line, c = "", 0
        while c < cols:
            n = min(random.randint(2, 7), cols - c)
            if mode == "dense":
                fg = random.randint(1, 255)
                bg = random.choice([None, None, random.randint(1, 255)])
                line += f"\x1b[38;5;{fg}m" + (f"\x1b[48;5;{bg}m" if bg is not None else "\x1b[49m")
            line += "".join(random.choice(chars) for _ in range(n)); c += n
        out.append(line + ("\x1b[0m" if mode == "dense" else "") + ("\r\n" if r < rows - 1 else ""))
    return "".join(out)
sys.stdout.write("\x1b[?7l\x1b[2J")          # no autowrap, clear
for i in range(frames):
    sys.stdout.write(screen()); sys.stdout.flush()
    if frames > 1: time.sleep(0.012)
sys.stdout.flush()
time.sleep(600)
