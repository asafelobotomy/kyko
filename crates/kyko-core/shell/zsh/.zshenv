# kyko: run the user's own .zshenv with ZDOTDIR put back, then keep ours for .zshrc.
__kyko_zdot="$ZDOTDIR"
if [ -n "$KYKO_USER_ZDOTDIR" ]; then ZDOTDIR="$KYKO_USER_ZDOTDIR"; else unset ZDOTDIR; fi
[ -r "${ZDOTDIR:-$HOME}/.zshenv" ] && . "${ZDOTDIR:-$HOME}/.zshenv"
KYKO_USER_ZDOTDIR="${ZDOTDIR:-}"
ZDOTDIR="$__kyko_zdot"
unset __kyko_zdot
