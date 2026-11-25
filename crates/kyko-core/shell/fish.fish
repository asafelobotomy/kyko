# kyko shell integration for fish. Sourced with --init-command, after the user's config.
# Loaded once per shell, and not exported (see bash.sh).
set -e KYKO_SHELL_INTEGRATION_LOADED
set -q __kyko_loaded; and exit 0
set -g __kyko_loaded 1

# fish 4 marks the prompt, command line and output itself (its `mark-prompt` feature), with the
# command line and click-to-move, and reports the folder: kyko then only adds what fish does not.
set -g __kyko_marks 1
status test-feature mark-prompt 2>/dev/null; and set -g __kyko_marks 0

function __kyko_prompt --on-event fish_prompt
    set -l s $status
    set -g __kyko_status $s
    test $__kyko_marks = 1; and set -q __kyko_ran; and printf '\e]133;D;%s\a' $s
    set -e __kyko_ran
    # fish 4 reports the folder itself.
    if test $__kyko_marks = 1
        printf '\e]7;file://%s%s\a' $hostname $PWD
        printf '\e]133;A\a'
    end
end
function __kyko_preexec --on-event fish_preexec
    set -g __kyko_ran 1
    test $__kyko_marks = 1; and printf '\e]133;C;cmdline_url=%s\a' (string escape --style=url -- "$argv[1]")
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
if test $__kyko_marks = 1; and functions -q fish_prompt
    functions -c fish_prompt __kyko_inner_prompt
    function fish_prompt
        __kyko_inner_prompt
        printf '\e]133;B\a'
    end
end

# fish already suggests as you type, searches history with Up and Down, completes in a menu and,
# with pkgfile installed, names the package of a missing command.
if test "$KYKO_EXTRAS" = 1
    type -q zoxide; and not functions -q __zoxide_z; and zoxide init fish | source
    type -q fzf; and not functions -q fzf-history-widget; and fzf --fish 2>/dev/null | source
    if type -q eza; and not functions -q ls
        alias ls 'eza --icons --group-directories-first'
    end
    if type -q bat; and not functions -q cat
        alias cat 'bat --paging=never'
    end
end
