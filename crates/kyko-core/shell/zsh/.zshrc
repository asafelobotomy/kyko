# kyko shell integration for zsh.
__kyko_user="${KYKO_USER_ZDOTDIR:-$HOME}"
ZDOTDIR="$KYKO_USER_ZDOTDIR"
[ -r "$__kyko_user/.zshrc" ] && . "$__kyko_user/.zshrc"
[ -n "$KYKO_USER_ZDOTDIR" ] || unset ZDOTDIR
unset __kyko_user
[ -n "$KYKO_SHELL_INTEGRATION_LOADED" ] && return 0
export KYKO_SHELL_INTEGRATION_LOADED=1

autoload -Uz add-zsh-hook
__kyko_precmd() {
    local s=$?
    [ -n "$__kyko_ran" ] && printf '\e]133;D;%s\a' "$s"
    __kyko_ran=
    printf '\e]7;file://%s%s\a' "${HOST:-}" "$PWD"
    printf '\e]133;A\a'
}
__kyko_preexec() { __kyko_ran=1; printf '\e]133;C\a' }
add-zsh-hook precmd __kyko_precmd
add-zsh-hook preexec __kyko_preexec
PS1="${PS1}%{$(printf '\e]133;B\a')%}"

if [ "$KYKO_PROMPT" = 1 ] && [ -z "$STARSHIP_SESSION_KEY" ] && command -v starship >/dev/null; then
    eval "$(starship init zsh)"
fi

if [ "$KYKO_EXTRAS" = 1 ]; then
    for p in /usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh \
             /usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh; do
        [ -r "$p" ] && . "$p"
    done
    command -v zoxide >/dev/null && eval "$(zoxide init zsh)"
    command -v fzf >/dev/null && eval "$(fzf --zsh 2>/dev/null)"
    if command -v eza >/dev/null && ! alias ls >/dev/null 2>&1; then alias ls='eza --icons --group-directories-first'; fi
    if command -v bat >/dev/null && ! alias cat >/dev/null 2>&1; then alias cat='bat --paging=never'; fi
fi
