# kyko shell integration for fish. Sourced with --init-command, after the user's config.
set -q KYKO_SHELL_INTEGRATION_LOADED; and exit 0
set -gx KYKO_SHELL_INTEGRATION_LOADED 1

function __kyko_prompt --on-event fish_prompt
    set -l s $status
    set -g __kyko_status $s
    set -q __kyko_ran; and printf '\e]133;D;%s\a' $s
    set -e __kyko_ran
    printf '\e]7;file://%s%s\a' (hostname) $PWD
    printf '\e]133;A\a'
end
function __kyko_preexec --on-event fish_preexec
    set -g __kyko_ran 1
    printf '\e]133;C\a'
end

# kyko draws the prompt only when the user has no fish_prompt of their own.
if test "$KYKO_PROMPT" = 1; and not set -q STARSHIP_SESSION_KEY; and type -q kyko-prompt
    if string match -rq '^(/usr(/local)?/share/fish|-$|stdin$)' -- (functions --details fish_prompt)
        function fish_prompt
            kyko-prompt render --shell fish --status $__kyko_status --duration $CMD_DURATION
        end
    end
end

# The end of the prompt is where input starts (OSC 133;B), whoever drew the prompt.
if functions -q fish_prompt
    functions -c fish_prompt __kyko_inner_prompt
    function fish_prompt
        __kyko_inner_prompt
        printf '\e]133;B\a'
    end
end

if test "$KYKO_EXTRAS" = 1
    type -q zoxide; and zoxide init fish | source
    type -q fzf; and fzf --fish 2>/dev/null | source
    if type -q eza; and not functions -q ls
        alias ls 'eza --icons --group-directories-first'
    end
    if type -q bat; and not functions -q cat
        alias cat 'bat --paging=never'
    end
end
