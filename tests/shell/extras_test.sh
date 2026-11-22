#!/usr/bin/env bash
# kyko's shell extras (KYKO_EXTRAS=1): defaults a bare zsh or bash gets, and that a user's own
# settings win. A shell that is not installed is skipped.
set -u
root=$(cd "$(dirname "$0")/../.." && pwd)
fail=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
check() { # name, got, want
    if [ "$2" = "$3" ]; then echo "ok: $1"; else echo "FAIL: $1: got [$2], want [$3]"; fail=1; fi
}
if command -v zsh >/dev/null; then
    zq() { # HOME's .zshrc, extras on or off, then what to print
        mkdir -p "$tmp/zh"; printf '%s\n' "$1" >"$tmp/zh/.zshrc"
        HOME="$tmp/zh" KYKO_EXTRAS=$2 KYKO_PROMPT=0 ZDOTDIR="$root/crates/kyko-core/shell/zsh" \
            zsh -i -c "$3" 2>/dev/null
    }
    check "zsh bare: history file" "$(zq '' 1 'print -r -- $HISTFILE $SAVEHIST')" "$tmp/zh/.zsh_history 50000"
    check "zsh bare: completion menu" "$(zq '' 1 'print ${+functions[compdef]}; zstyle -L ":completion:*" menu')" \
        "1
zstyle ':completion:*' menu select"
    check "zsh bare: Home key" "$(zq '' 1 'bindkey "^[[H"')" '"^[[H" beginning-of-line'
    check "zsh: the user's history kept" "$(zq 'HISTFILE=~/mine; SAVEHIST=7' 1 'print -r -- $HISTFILE $SAVEHIST')" "$tmp/zh/mine 7"
    check "zsh: the user's key kept" "$(zq 'bindkey "^[[A" up-line-or-search' 1 'bindkey "^[[A"')" '"^[[A" up-line-or-search'
    check "zsh: extras off" "$(zq '' 0 'print -r -- "[$HISTFILE]" ${+functions[compdef]}')" "[] 0"
    # A command line keeps its markup: the zsh error after every command is gone.
    out=$(zq '' 1 '__kyko_t0=$EPOCHREALTIME; __kyko_ran=1; __kyko_precmd 2>&1 >/dev/null')
    check "zsh: precmd runs cleanly" "$out" ""
fi
if command -v bash >/dev/null; then
    bq() {
        mkdir -p "$tmp/bh"; printf '%s\n' "$1" >"$tmp/bh/.bashrc"
        HOME="$tmp/bh" KYKO_EXTRAS=$2 KYKO_PROMPT=0 bash --rcfile "$root/crates/kyko-core/shell/bash.sh" -i -c "$3" 2>/dev/null
    }
    check "bash bare: history" "$(bq '' 1 'echo $HISTSIZE $HISTCONTROL')" "50000 ignoreboth"
    check "bash: the user's history kept" "$(bq 'HISTSIZE=900' 1 'echo $HISTSIZE')" "900"
    check "bash: extras off" "$(bq '' 0 'echo ${HISTCONTROL:-none}')" "none"
fi
exit $fail
