# zz zsh startup bridge. Restore the user's ZDOTDIR before zsh proceeds to
# .zprofile/.zshrc, while sourcing the title hooks from zz's private cache.

typeset -g __zz_integration_dir=$ZZ_SHELL_INTEGRATION_DIR
if [[ -n ${ZZ_ZSH_ZDOTDIR+x} ]]; then
  builtin export ZDOTDIR=$ZZ_ZSH_ZDOTDIR
  builtin unset ZZ_ZSH_ZDOTDIR
else
  builtin unset ZDOTDIR
fi
builtin unset ZZ_SHELL_INTEGRATION_DIR ZZ_SHELL_INTEGRATION_ACTIVE

typeset -g __zz_user_zshenv=${ZDOTDIR-$HOME}/.zshenv
[[ ! -r $__zz_user_zshenv ]] || builtin source -- "$__zz_user_zshenv"
if [[ -o interactive && -r $__zz_integration_dir/zsh/zz-integration.zsh ]]; then
  builtin source -- "$__zz_integration_dir/zsh/zz-integration.zsh"
fi
builtin unset __zz_user_zshenv __zz_integration_dir
