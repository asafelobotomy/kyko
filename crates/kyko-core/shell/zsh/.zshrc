# kyko shell integration for zsh.
__kyko_prompt_system="$PROMPT"
__kyko_user="${KYKO_USER_ZDOTDIR:-$HOME}"
ZDOTDIR="$KYKO_USER_ZDOTDIR"
[ -r "$__kyko_user/.zshrc" ] && . "$__kyko_user/.zshrc"
[ -n "$KYKO_USER_ZDOTDIR" ] || unset ZDOTDIR
unset __kyko_user
[ -n "$KYKO_SHELL_INTEGRATION_LOADED" ] && return 0
export KYKO_SHELL_INTEGRATION_LOADED=1

autoload -Uz add-zsh-hook
zmodload zsh/datetime 2>/dev/null
__kyko_draw=
if [ "$KYKO_PROMPT" = 1 ] && [ "$PROMPT" = "$__kyko_prompt_system" ] && [ -z "$STARSHIP_SESSION_KEY" ] \
    && command -v kyko-prompt >/dev/null; then
    __kyko_draw=1
fi
__kyko_precmd() {
    local s=$?
    [ -n "$__kyko_ran" ] && printf '\e]133;D;%s\a' "$s"
    local ms=0
    [ -n "$__kyko_t0" ] && ms=$(( int((EPOCHREALTIME - __kyko_t0) * 1000) ))
    __kyko_ran= __kyko_t0=
    printf '\e]7;file://%s%s\a' "${HOST:-}" "$PWD"
    printf '\e]133;A\a'
    [ -n "$__kyko_draw" ] && PROMPT="$(kyko-prompt render --shell zsh --status "$s" --duration "$ms")%{$(printf '\e]133;B\a')%}"
}
__kyko_preexec() { __kyko_ran=1; __kyko_t0=$EPOCHREALTIME; printf '\e]133;C\a' }
add-zsh-hook precmd __kyko_precmd
add-zsh-hook preexec __kyko_preexec
[ -n "$__kyko_draw" ] || PS1="${PS1}%{$(printf '\e]133;B\a')%}"

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
