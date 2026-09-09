# zz zsh shell integration. Installation is deferred until the first prompt so
# user startup files and prompt frameworks finish configuring their hooks first.

[[ -o interactive ]] || return 0
(( ! ${+__ZZ_TITLE_INTEGRATION_INSTALLED} )) || return 0
typeset -gi __ZZ_TITLE_INTEGRATION_INSTALLED=1

_zz_write_title() {
  emulate -L zsh
  local value=${1-}
  value=${value//$'\n'/ }
  value=${value//$'\r'/ }
  value=${value//[[:cntrl:]]/}
  value=${value[1,512]}
  print -rn -- $'\e]2;'$value$'\a'
}

_zz_write_working_directory() {
  emulate -L zsh
  local value=$PWD
  value=${value//$'\n'/}
  value=${value//$'\r'/}
  value=${value//[[:cntrl:]]/}
  value=${value[1,4096]}
  print -rn -- $'\e]7;file://'$HOST$value$'\a'
}

_zz_title_precmd() {
  local zz_status=${1:-$?}
  emulate -L zsh
  if (( ${_zz_command_running:-0} )); then
    print -rn -- $'\e]133;D;'$zz_status$'\a'
  fi
  typeset -gi _zz_command_running=0
  local shell_name=${ZSH_NAME:-zsh}
  _zz_write_working_directory
  _zz_write_title "$shell_name"
  # DECSCUSR default: the prompt restores the configured cursor style instead of
  # imposing one, so a program that exits without resetting cannot keep its shape.
  print -rn -- $'\e[0 q'
  [[ $PS1 == *$'%{\e]133;B\a%}' ]] || PS1+=$'%{\e]133;B\a%}'
  print -rn -- $'\e]133;A\a'
}

_zz_title_preexec() {
  emulate -L zsh
  print -rn -- $'\e[0 q'
  _zz_write_title "${1-}"
  typeset -gi _zz_command_running=1
  print -rn -- $'\e]133;C\a'
}

_zz_install_title_hooks() {
  local zz_status=$?
  emulate -L zsh
  typeset -ga precmd_functions preexec_functions
  precmd_functions=(${precmd_functions:#_zz_install_title_hooks})
  (( ${precmd_functions[(I)_zz_title_precmd]} )) || precmd_functions+=(_zz_title_precmd)
  (( ${preexec_functions[(I)_zz_title_preexec]} )) || preexec_functions+=(_zz_title_preexec)
  _zz_title_precmd "$zz_status"
}

typeset -ga precmd_functions
precmd_functions+=(_zz_install_title_hooks)
