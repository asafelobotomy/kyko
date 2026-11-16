# kyko shell integration for bash. Loaded with --rcfile: runs the user's own rc first.
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
[ -n "$KYKO_SHELL_INTEGRATION_LOADED" ] && return 0
export KYKO_SHELL_INTEGRATION_LOADED=1
__kyko_dir="${BASH_SOURCE[0]%/*}"

__kyko_precmd() {
    local s=$?
    [ -n "$__kyko_ran" ] && printf '\e]133;D;%s\a' "$s"
    __kyko_ran=
    printf '\e]7;file://%s%s\a' "${HOSTNAME:-}" "$PWD"
    printf '\e]133;A\a'
    return $s
}
__kyko_preexec() {
    [ -n "$COMP_LINE" ] && return
    [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ] && return
    case "$BASH_COMMAND" in __kyko_*) return ;; esac
    if [ -z "$__kyko_ran" ]; then
        __kyko_ran=1
        printf '\e]133;C\a'
    fi
}
PROMPT_COMMAND="__kyko_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
trap '__kyko_preexec' DEBUG
PS1="${PS1}\[\e]133;B\a\]"

# A prompt of the user's own wins: kyko only adds the marks above.
if [ "$KYKO_PROMPT" = 1 ] && [ -z "$STARSHIP_SESSION_KEY" ] && command -v starship >/dev/null; then
    eval "$(starship init bash)"
fi

if [ "$KYKO_EXTRAS" = 1 ]; then
    command -v zoxide >/dev/null && eval "$(zoxide init bash)"
    command -v fzf >/dev/null && eval "$(fzf --bash 2>/dev/null)"
    if command -v eza >/dev/null && ! alias ls >/dev/null 2>&1; then alias ls='eza --icons --group-directories-first'; fi
    if command -v bat >/dev/null && ! alias cat >/dev/null 2>&1; then alias cat='bat --paging=never'; fi
fi
