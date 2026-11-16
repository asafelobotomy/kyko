# kyko: the shell experience

Date: 2026-11-16 · Status: approach chosen by the owner on 2026-11-16 ("kyko prompt + shell
integration"); details below are a proposal to build against.
Related: `2026-11-10-kyko-design.md` (§2.4 terminal, §2.8 holder, §2.12 default terminal).

## 1. Intent

A terminal that is the default for everything should be clear at a glance and pleasant in every
shell. fish is nice because its prompt, colours and helpers are good out of the box; kyko gives
that to **bash, zsh and fish alike**, from one place, instead of per shell:

- a **prompt kyko draws** (`kyko-prompt`), the same in every shell, in kyko's theme colours, with
  icons and markers;
- **shell integration**: the shell tells kyko where prompts, commands and their output are, what
  the exit status was, and what the working folder is, so kyko can do things a plain terminal
  cannot;
- **everyday conveniences** in the terminal and in sane shell defaults.

Everything is on by default inside kyko, can be turned off in Preferences (a new **Shell** page),
and never touches the user's own files: kyko loads the user's rc files first and adds its layer on
top, only in kyko's own sessions.

## 2. Shell integration

### 2.1 What the shell sends

Standard sequences, so other terminals' behaviour is unchanged and kyko's parser can be tested
against real shells:

| Sequence | Meaning | Used for |
|---|---|---|
| `OSC 133;A` | prompt starts | prompt marks, jump between prompts |
| `OSC 133;B` | prompt ends, input starts | click to move the cursor at the prompt |
| `OSC 133;C` | command starts running | running state, command text, timer |
| `OSC 133;D;<code>` | command finished with exit status | status mark, notification |
| `OSC 7;file://host/path` | working folder | new tabs and splits start there |
| `OSC 2` | title | tab name (`program` while running, folder at the prompt) |

kykod's parser records marks in the pane's scrollback (a mark is a line number, kind and exit
status, kept in step when lines scroll off), so they survive the window being closed and reopened.

### 2.2 How it gets into the shell

kyko starts a shell with `KYKO=1` and `KYKO_SHELL_INTEGRATION=<dir>` in its environment and the
integration files in `<dir>` (written to `$XDG_RUNTIME_DIR/kyko/shell`, from copies compiled into
the binary, so a package upgrade never leaves stale scripts):

- **bash**: started as `bash --rcfile <dir>/bash.sh -i`; `bash.sh` sources `/etc/bash.bashrc` and
  `~/.bashrc` first, then installs `PROMPT_COMMAND` and a `DEBUG` trap (kept compatible with
  bash-preexec if the user has it).
- **zsh**: `ZDOTDIR=<dir>/zsh` whose startup files source the user's own (`$KYKO_USER_ZDOTDIR`)
  and then add `precmd`/`preexec` hooks.
- **fish**: `XDG_DATA_DIRS` gains `<dir>/fish`, whose `vendor_conf.d/kyko.fish` runs after
  `config.fish` and adds event handlers.
- **anything else** (dash, nu, xonsh…): no integration, `OSC 7`-less, still works; the cwd comes
  from `/proc` as today.

`shell.integration = false` skips all of this; `KYKO_SHELL_INTEGRATION` is unset in nested ssh/sudo
sessions on purpose.

## 3. The prompt

`kyko-prompt` is a small Rust binary shipped with kyko (`/usr/bin/kyko-prompt`). The shell hooks
call `kyko-prompt render --shell bash --status $? --duration-ms N --jobs N` and print what it
returns. It must answer in under 20 ms on a warm cache; git state is the slow part, so it asks
`git status --porcelain=v2 --branch` with a 150 ms limit and shows nothing for git when it takes
longer.

### 3.1 Look

```
╭─  ~/code/kyko   main ✚2 ⇡1                                   ✔  3.2s  14:07
╰─❯
```

- **Line 1, left**: a coloured pill with the folder (home is `~`, long paths shortened to the
  repo root and the last two folders), then the git branch with a state glyph (staged, changed,
  untracked, ahead and behind, stash). Pill colours come from kyko's theme tokens.
- **Line 1, right**: exit status (✔ green, ✘ red with the code), how long the last command took
  when over 2 seconds, the time, and background jobs when there are any. Right-aligned with the
  shell's own right-prompt mechanism where it has one, else padded.
- **Line 2**: the input line, `❯` in the status colour (accent when fine, red after a failure).
- **Context chips, only when relevant**: `user@host` over ssh or as root, a lock icon in a folder
  you cannot write, the active Python venv, Node/Rust/Go toolchain versions when the folder is a
  project of that kind, `nix-shell`/`toolbox`/`distrobox` names.
- **Transient prompt** (on by default): when a command is entered, its prompt collapses to the
  short form `❯ command` so the scrollback stays easy to read.
- Icons need a symbol font. kyko ships *Symbols Nerd Font Mono* (MIT) as a terminal fallback
  font, so the glyphs work even when the user's font has none. `prompt.icons = "nerd" | "unicode" |
  "ascii"` (default `nerd` inside kyko; `kyko-prompt` picks `unicode` when run elsewhere).
- Colours: truecolor from the active theme (the same tokens as the window), passed in
  `KYKO_PALETTE` and refreshed on theme change, so the prompt follows uQ's theme live. Plain 16
  colours when `NO_COLOR` is set.

### 3.2 Respecting what the user has

`shell.prompt = "kyko"` (default) replaces the prompt in kyko sessions after the user's rc files
ran; `"own"` leaves the user's prompt alone and keeps the integration marks only (the marks are
added to a foreign prompt through `PS1` wrapping, which works for plain prompts and for starship,
p10k and oh-my-posh). Choosing `own` is one switch on the Shell page.

## 4. Terminal conveniences

All of these use the marks and work in every shell with integration on.

1. **Prompt gutter**: a thin coloured bar left of each command, green when it succeeded, red when
   it failed, and a spinner while it runs.
2. **Jump between prompts**: `Ctrl+Shift+↑/↓` scroll to the previous or next prompt.
3. **Copy last output**: `Ctrl+Shift+O` selects the last command's output, `Ctrl+Shift+Alt+O`
   copies it straight away.
4. **Long command finished**: when a command that ran over 10 s ends while kyko is hidden or
   unfocused, a desktop notification names the command and the exit status (off in Preferences).
5. **New tabs and splits start in the focused folder** (already `terminal.new_tab_dir = current`;
   it now reads `OSC 7`, so it also works through shells, sudo and containers).
6. **Tab names**: the running command while one runs, the folder at the prompt.
7. **Click to place the cursor** at the prompt (sends the arrow keys needed).
8. **Ctrl+click** opens URLs and `path:line` references (planned in v0.2.0 already).
9. **Multi-line paste confirmation** (planned in v0.2.0 already).
10. **Shell defaults that make sense**, in kyko sessions only, with `shell.defaults = false` to
    opt out: bash and zsh get a large shared history without duplicates, case-insensitive
    completion with a menu, no terminal bell on completion, `cd` by folder name in zsh,
    globstar in bash; fish already has these. Plus a few small helper commands in all three:
    `kyko-copy` (pipe to the clipboard), `kyko-open` (open in the desktop's app), `kyko-notify`
    (notify when a command ends).

Not in scope: autosuggestions and syntax highlighting for bash and zsh (heavy, shell-specific, and
fish has them); a plugin manager; changing the user's dotfiles.

## 5. Preferences: the Shell page

`shell.integration`, `shell.prompt` (kyko / own), `prompt.icons`, `prompt.two_line`,
`prompt.transient`, `prompt.show_time`, `prompt.show_duration_over_ms`, `shell.defaults`,
`shell.notify_after_s`. A live preview of the prompt sits above the controls.

## 6. Testing

- kyko-term / kykod unit tests for the mark parser (marks stay correct as lines scroll, resize,
  alt screen, partial sequences, a hostile program printing marks of its own).
- A shell test per shell that starts the real shell in a pty with the integration, runs a few
  commands (success, failure, long, empty line, Ctrl+C) and checks the marks and exit statuses
  that come back; zsh and fish are installed in CI for it.
- `kyko-prompt` golden tests per segment (git states built in a temp repo, ssh, root, jobs,
  narrow widths, ASCII/unicode/nerd modes, `NO_COLOR`).
- A headless-window test and screenshot for the gutter, jump and copy-last-output.

## 7. Slices

1. **Marks**: parser for `OSC 133/7`, marks in kykod, cwd from `OSC 7` for new tabs and tab names,
   the integration scripts for bash, zsh and fish, and the shell tests.
2. **kyko-prompt** and the hooks that draw it, with the transient prompt and the palette.
3. **Window**: gutter, jump, copy last output, finished-command notification, click to place the
   cursor.
4. **Shell page** in Preferences and the sane defaults and helper commands.

Slices 1 and 2 are the part the owner will see first. All four are meant for 0.2.0 or 0.3.0; the
1.0.0 gate in `docs/releasing.md` is unchanged.
