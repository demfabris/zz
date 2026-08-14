# zz Bash shell integration. This file is generated into a private cache and
# sourced through Bash's POSIX ENV startup path.

[[ $- == *i* ]] || builtin return 0
[[ -z ${__ZZ_TITLE_INTEGRATION_INSTALLED-} ]] || builtin return 0
builtin declare -g __ZZ_TITLE_INTEGRATION_INSTALLED=1

if [[ -n ${ZZ_BASH_INJECT-} ]]; then
  builtin unset ENV ZZ_BASH_INJECT

  if [[ -n ${ZZ_BASH_ENV+x} ]]; then
    builtin export ENV="$ZZ_BASH_ENV"
    builtin unset ZZ_BASH_ENV
  fi

  builtin set +o posix
  builtin shopt -u inherit_errexit 2>/dev/null || true

  if [[ -n ${ZZ_BASH_UNEXPORT_HISTFILE-} ]]; then
    builtin export -n HISTFILE
    builtin unset ZZ_BASH_UNEXPORT_HISTFILE
  fi

  if builtin shopt -q login_shell; then
    [[ ! -r /etc/profile ]] || builtin source /etc/profile
    for __zz_startup_file in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
      if [[ -r "$__zz_startup_file" ]]; then
        builtin source "$__zz_startup_file"
        break
      fi
    done
  else
    for __zz_startup_file in /etc/bash.bashrc /etc/bash/bashrc /etc/bashrc; do
      if [[ -r "$__zz_startup_file" ]]; then
        builtin source "$__zz_startup_file"
        break
      fi
    done
    [[ ! -r "$HOME/.bashrc" ]] || builtin source "$HOME/.bashrc"
  fi
  builtin unset __zz_startup_file
fi

__zz_write_title() {
  builtin local __zz_value="${1-}"
  __zz_value=${__zz_value//$'\n'/ }
  __zz_value=${__zz_value//$'\r'/ }
  __zz_value=${__zz_value//[[:cntrl:]]/}
  __zz_value=${__zz_value:0:512}
  builtin printf '\e]2;%s\a' "$__zz_value"
}

__zz_write_working_directory() {
  builtin local __zz_value="$PWD"
  __zz_value=${__zz_value//$'\n'/}
  __zz_value=${__zz_value//$'\r'/}
  __zz_value=${__zz_value//[[:cntrl:]]/}
  __zz_value=${__zz_value:0:4096}
  builtin printf '\e]7;file://%s%s\a' "${HOSTNAME-}" "$__zz_value"
}

__zz_title_precmd() {
  __zz_write_working_directory
  __zz_write_title "${BASH##*/}"
  # DECSCUSR default: the prompt restores the configured cursor style instead of
  # imposing one, so a program that exits without resetting cannot keep its shape.
  builtin printf '\e[0 q'
}

__zz_title_preexec() {
  builtin local __zz_history_line='' __zz_command=''
  builtin printf '\e[0 q'
  __zz_history_line=$(LC_ALL=C HISTTIMEFORMAT='' builtin history 1)
  if [[ $__zz_history_line =~ ^[[:space:]]*[0-9]+[[:space:]]+(.*)$ ]]; then
    __zz_command=${BASH_REMATCH[1]}
  else
    __zz_command=$__zz_history_line
  fi
  [[ -z $__zz_command ]] || __zz_write_title "$__zz_command"
}

if (( BASH_VERSINFO[0] > 5 || (BASH_VERSINFO[0] == 5 && BASH_VERSINFO[1] >= 3) )); then
  # shellcheck disable=SC2016 # Bash expands this when PS0 is displayed.
  [[ $PS0 == *'__zz_title_preexec'* ]] || PS0='${ __zz_title_preexec; }'"${PS0-}"
elif (( BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4) )); then
  # shellcheck disable=SC2016 # Bash expands this when PS0 is displayed.
  [[ $PS0 == *'__zz_title_preexec'* ]] || PS0='$(__zz_title_preexec >/dev/tty)'"${PS0-}"
fi

if [[ $(builtin declare -p PROMPT_COMMAND 2>/dev/null) == 'declare -a '* ]]; then
  [[ " ${PROMPT_COMMAND[*]} " == *' __zz_title_precmd '* ]] || PROMPT_COMMAND+=(__zz_title_precmd)
elif [[ -z ${PROMPT_COMMAND-} ]]; then
  PROMPT_COMMAND=__zz_title_precmd
elif [[ $PROMPT_COMMAND != *'__zz_title_precmd'* ]]; then
  PROMPT_COMMAND+=';__zz_title_precmd'
fi

builtin unset ZZ_SHELL_INTEGRATION_ACTIVE
