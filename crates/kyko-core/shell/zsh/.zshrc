# kyko shell integration for zsh.
__kyko_prompt_system="$PROMPT"
__kyko_user="${KYKO_USER_ZDOTDIR:-$HOME}"
ZDOTDIR="$KYKO_USER_ZDOTDIR"
[ -r "$__kyko_user/.zshrc" ] && . "$__kyko_user/.zshrc"
[ -n "$KYKO_USER_ZDOTDIR" ] || unset ZDOTDIR
# Loaded once per shell, and not exported (see bash.sh).
unset KYKO_SHELL_INTEGRATION_LOADED
[ -n "$__kyko_loaded" ] && { unset __kyko_user; return 0 }
typeset -g __kyko_loaded=1

autoload -Uz add-zsh-hook
zmodload zsh/datetime 2>/dev/null
__kyko_draw=
if [ "$KYKO_PROMPT" = 1 ] && [ "$PROMPT" = "$__kyko_prompt_system" ] && [ -z "$STARSHIP_SESSION_KEY" ] \
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
    [ -n "$__kyko_ran" ] && printf '\e]133;D;%s\a' "$s"
    local -i ms=0
    # An integer variable drops the fraction (`int()` would need zsh/mathfunc).
    [ -n "$__kyko_t0" ] && (( ms = (EPOCHREALTIME - __kyko_t0) * 1000 ))
    __kyko_ran= __kyko_t0=
    printf '\e]7;file://%s%s\a' "${HOST:-}" "$PWD"
    printf '\e]133;A\a'
    [ -n "$__kyko_draw" ] && PROMPT="$(kyko-prompt render --shell zsh --status "$s" --duration "$ms")%{$(printf '\e]133;B\a')%}"
}
__kyko_preexec() {
    __kyko_ran=1
    __kyko_t0=$EPOCHREALTIME
    local REPLY
    __kyko_url "$1"
    printf '\e]133;C;cmdline_url=%s\a' "$REPLY"
}
add-zsh-hook precmd __kyko_precmd
add-zsh-hook preexec __kyko_preexec
[ -n "$__kyko_draw" ] || PS1="${PS1}%{$(printf '\e]133;B\a')%}"

if [ "$KYKO_EXTRAS" = 1 ]; then
    # History: kept and shared between tabs, when the user's files set no history file.
    if [ -z "$HISTFILE" ]; then
        HISTFILE="$__kyko_user/.zsh_history"
        (( HISTSIZE > 30 )) || HISTSIZE=50000
        (( SAVEHIST )) || SAVEHIST=50000
        setopt share_history extended_history hist_ignore_dups hist_ignore_space hist_reduce_blanks
    fi
    # A completion menu: compinit when the user's files did not run it, styles they did not set.
    if (( ! ${+functions[compdef]} )); then
        __kyko_cache="${XDG_CACHE_HOME:-$HOME/.cache}/zsh"
        mkdir -p "$__kyko_cache" 2>/dev/null
        autoload -Uz compinit && compinit -d "$__kyko_cache/zcompdump-$ZSH_VERSION"
        unset __kyko_cache
    fi
    if ! zstyle -m ':completion:*' menu '*'; then
        zmodload zsh/complist 2>/dev/null
        zstyle ':completion:*' menu select
        zstyle ':completion:*' matcher-list 'm:{a-zA-Z}={A-Za-z}' 'r:|[._-]=* r:|=*'
        zstyle ':completion:*' list-colors ${(s.:.)LS_COLORS}
        zstyle ':completion:*' group-name ''
        zstyle ':completion:*:descriptions' format '%F{yellow}%d%f'
    fi
    # Home, End, Delete and Ctrl+arrows, where no key map binds them.
    for __kyko_map in emacs viins; do
        for __kyko_key __kyko_w in '^[[H' beginning-of-line '^[OH' beginning-of-line \
            '^[[F' end-of-line '^[OF' end-of-line '^[[3~' delete-char \
            '^[[1;5C' forward-word '^[[1;5D' backward-word '^[[3;5~' kill-word; do
            [[ "$(bindkey -M $__kyko_map "$__kyko_key")" == *undefined-key* ]] \
                && bindkey -M $__kyko_map "$__kyko_key" $__kyko_w
        done
    done
    unset __kyko_map __kyko_key __kyko_w
    command -v zoxide >/dev/null && (( ! ${+functions[__zoxide_z]} )) && eval "$(zoxide init zsh)"
    command -v fzf >/dev/null && (( ! ${+functions[fzf-history-widget]} )) && eval "$(fzf --zsh 2>/dev/null)"
    # Plugins, if installed and not loaded already. fzf-tab must come after compinit and before
    # the plugins that wrap widgets (so not when the user's files loaded those);
    # history-substring-search after syntax highlighting.
    if (( ! ${+functions[enable-fzf-tab]} && ! ${+functions[_zsh_autosuggest_start]} \
        && ! ${+functions[_zsh_highlight]} )); then
        for p in /usr/share/zsh/plugins/fzf-tab{,-git}/fzf-tab.plugin.zsh; do
            [ -r "$p" ] && { . "$p"; break }
        done
    fi
    p=/usr/share/zsh/plugins/zsh-autosuggestions/zsh-autosuggestions.zsh
    (( ${+functions[_zsh_autosuggest_start]} )) || { [ -r "$p" ] && . "$p" }
    p=/usr/share/zsh/plugins/zsh-syntax-highlighting/zsh-syntax-highlighting.zsh
    (( ${+functions[_zsh_highlight]} )) || { [ -r "$p" ] && . "$p" }
    p=/usr/share/zsh/plugins/zsh-history-substring-search/zsh-history-substring-search.zsh
    if (( ! ${+functions[history-substring-search-up]} )) && [ -r "$p" ]; then
        . "$p"
        # Up and Down search by what is typed, unless the user bound them to something else.
        for __kyko_map in emacs viins; do
            for k in '^[[A' '^[OA'; do
                [[ "$(bindkey -M $__kyko_map "$k")" == *" up-line-or-history" ]] \
                    && bindkey -M $__kyko_map "$k" history-substring-search-up
            done
            for k in '^[[B' '^[OB'; do
                [[ "$(bindkey -M $__kyko_map "$k")" == *" down-line-or-history" ]] \
                    && bindkey -M $__kyko_map "$k" history-substring-search-down
            done
        done
        unset __kyko_map k
    fi
    unset p
    # A missing command names the package it is in (pkgfile).
    if command -v pkgfile >/dev/null && (( ! ${+functions[command_not_found_handler]} )) \
        && [ -r /usr/share/doc/pkgfile/command-not-found.zsh ]; then
        . /usr/share/doc/pkgfile/command-not-found.zsh
    fi
    if command -v eza >/dev/null && ! alias ls >/dev/null 2>&1; then alias ls='eza --icons --group-directories-first'; fi
    if command -v bat >/dev/null && ! alias cat >/dev/null 2>&1; then alias cat='bat --paging=never'; fi
fi
unset __kyko_user
