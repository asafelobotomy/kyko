# kyko shell integration for bash. Loaded with --rcfile, which skips the system rc: run it,
# then the user's own, as a plain interactive bash would.
[ -r /etc/bash.bashrc ] && . /etc/bash.bashrc
__kyko_ps1_system="$PS1"
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
[ -n "$KYKO_SHELL_INTEGRATION_LOADED" ] && return 0
export KYKO_SHELL_INTEGRATION_LOADED=1

# kyko draws the prompt only when nothing set one of the user's own.
__kyko_draw=
if [ "$KYKO_PROMPT" = 1 ] && [ "$PS1" = "$__kyko_ps1_system" ] && [ -z "$STARSHIP_SESSION_KEY" ] \
    && command -v kyko-prompt >/dev/null; then
    __kyko_draw=1
fi

__kyko_precmd() {
    local s=$?
    [ -n "$__kyko_ran" ] && printf '\e]133;D;%s\a' "$s"
    local ms=0
    [ -n "$__kyko_t0" ] && ms=$(( (${EPOCHREALTIME/./} - __kyko_t0) / 1000 ))
    __kyko_ran= __kyko_t0=
    printf '\e]7;file://%s%s\a' "${HOSTNAME:-}" "$PWD"
    printf '\e]133;A\a'
    [ -n "$__kyko_draw" ] && PS1="$(kyko-prompt render --shell bash --status "$s" --duration "$ms")"$'\001\e]133;B\a\002'
    return $s
}
__kyko_preexec() {
    [ -n "$COMP_LINE" ] && return
    [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ] && return
    case "$BASH_COMMAND" in __kyko_*) return ;; esac
    if [ -z "$__kyko_ran" ]; then
        __kyko_ran=1
        __kyko_t0=${EPOCHREALTIME/./}
        printf '\e]133;C\a'
    fi
}
PROMPT_COMMAND="__kyko_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
trap '__kyko_preexec' DEBUG
[ -n "$__kyko_draw" ] || PS1="${PS1}\[\e]133;B\a\]"

if [ "$KYKO_EXTRAS" = 1 ]; then
    command -v zoxide >/dev/null && eval "$(zoxide init bash)"
    command -v fzf >/dev/null && eval "$(fzf --bash 2>/dev/null)"
    if command -v eza >/dev/null && ! alias ls >/dev/null 2>&1; then alias ls='eza --icons --group-directories-first'; fi
    if command -v bat >/dev/null && ! alias cat >/dev/null 2>&1; then alias cat='bat --paging=never'; fi
fi
