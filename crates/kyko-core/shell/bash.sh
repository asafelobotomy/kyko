# kyko shell integration for bash. Loaded with --rcfile, which skips the system rc: run it,
# then the user's own, as a plain interactive bash would.
[ -r /etc/bash.bashrc ] && . /etc/bash.bashrc
__kyko_ps1_system="$PS1"
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
# Loaded once per shell. Not exported: a bash this shell starts (makepkg's check() runs kyko's
# own tests) must not inherit "already loaded". An older kyko exported it; drop that too.
unset KYKO_SHELL_INTEGRATION_LOADED
[ -n "$__kyko_loaded" ] && return 0
__kyko_loaded=1

# kyko draws the prompt only when nothing set one of the user's own.
__kyko_draw=
if [ "$KYKO_PROMPT" = 1 ] && [ "$PS1" = "$__kyko_ps1_system" ] && [ -z "$STARSHIP_SESSION_KEY" ] \
    && command -v kyko-prompt >/dev/null; then
    __kyko_draw=1
fi

# $1 percent-encoded enough for an OSC parameter, into REPLY.
__kyko_url() {
    REPLY=${1//\%/%25}
    REPLY=${REPLY//;/%3B}
    REPLY=${REPLY//$'\n'/%0A}
    REPLY=${REPLY//$'\e'/}
    REPLY=${REPLY//$'\a'/}
}

__kyko_precmd() {
    local s=$?
    __kyko_in_prompt=1
    [ -n "$__kyko_ran" ] && printf '\e]133;D;%s\a' "$s"
    local ms=0
    [ -n "$__kyko_t0" ] && ms=$(( (${EPOCHREALTIME/./} - __kyko_t0) / 1000 ))
    __kyko_ran= __kyko_t0=
    printf '\e]7;file://%s%s\a' "${HOSTNAME:-}" "$PWD"
    printf '\e]133;A\a'
    [ -n "$__kyko_draw" ] && PS1="$(kyko-prompt render --shell bash --status "$s" --duration "$ms")"$'\001\e]133;B\a\002'
    return $s
}
# Ends the prompt commands: what runs after it is the user's.
__kyko_prompt_done() {
    __kyko_in_prompt=
}
__kyko_preexec() {
    [ -n "$COMP_LINE" ] && return
    [ -n "$__kyko_in_prompt" ] && return
    [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ] && return
    case "$BASH_COMMAND" in __kyko_*) return ;; esac
    if [ -z "$__kyko_ran" ]; then
        __kyko_ran=1
        __kyko_t0=${EPOCHREALTIME/./}
        # The line just run is the newest history entry, unless it was kept out of the history
        # (a leading space, a duplicate): then the entry is the one seen last time.
        local h REPLY=
        h=$(HISTTIMEFORMAT= builtin history 1)
        if [ -n "$h" ] && [ "$h" != "$__kyko_last_history" ]; then
            __kyko_last_history=$h
            h=${h#*[0-9] }
            __kyko_url "${h#"${h%%[![:space:]]*}"}"
        fi
        printf '\e]133;C%s\a' "${REPLY:+;cmdline_url=$REPLY}"
    fi
}

if [ "$KYKO_EXTRAS" = 1 ]; then
    # History: kept, long, shared between tabs. Only where the user's files left bash's defaults.
    [ "${HISTSIZE:-500}" -le 500 ] 2>/dev/null && HISTSIZE=50000
    [ "${HISTFILESIZE:-500}" -le 500 ] 2>/dev/null && HISTFILESIZE=100000
    [ -z "$HISTCONTROL" ] && HISTCONTROL=ignoreboth
    shopt -s histappend checkwinsize
    # Completion and Up/Down history search, when the user has no readline file of their own.
    if [ -z "$INPUTRC" ] && [ ! -e "$HOME/.inputrc" ]; then
        bind 'set completion-ignore-case on'
        bind 'set show-all-if-ambiguous on'
        bind 'set colored-stats on'
        bind 'set mark-symlinked-directories on'
        for k in '\e[A' '\eOA'; do bind "\"$k\": history-search-backward"; done
        for k in '\e[B' '\eOB'; do bind "\"$k\": history-search-forward"; done
    fi
    # A missing command names the package it is in (pkgfile).
    if command -v pkgfile >/dev/null && ! declare -F command_not_found_handle >/dev/null \
        && [ -r /usr/share/doc/pkgfile/command-not-found.bash ]; then
        . /usr/share/doc/pkgfile/command-not-found.bash
    fi
    command -v zoxide >/dev/null && ! declare -F __zoxide_z >/dev/null && eval "$(zoxide init bash)"
    command -v fzf >/dev/null && ! declare -F __fzf_history__ >/dev/null && eval "$(fzf --bash 2>/dev/null)"
    if command -v eza >/dev/null && ! alias ls >/dev/null 2>&1; then alias ls='eza --icons --group-directories-first'; fi
    if command -v bat >/dev/null && ! alias cat >/dev/null 2>&1; then alias cat='bat --paging=never'; fi
fi

# Last, after anything above added its own prompt commands (zoxide does).
# PROMPT_COMMAND may be an array (bash 5.1; Arch's bash.bashrc adds to it): kyko's own commands
# go first and last, so the DEBUG trap can tell the prompt's commands from the user's.
PROMPT_COMMAND=(__kyko_precmd "${PROMPT_COMMAND[@]}" __kyko_prompt_done)
[ -n "$__kyko_draw" ] || PS1="${PS1}\[\e]133;B\a\]"
__kyko_last_history=$(HISTTIMEFORMAT= builtin history 1)
# Until the first prompt, nothing that runs is a command of the user's.
__kyko_in_prompt=1
trap '__kyko_preexec' DEBUG
