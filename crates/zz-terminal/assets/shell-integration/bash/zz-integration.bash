# zz Bash shell integration. This file is generated into a private cache and
# sourced through Bash's POSIX ENV startup path.

[[ $- == *i* ]] || builtin return 0
[[ -z ${__ZZ_TITLE_INTEGRATION_INSTALLED-} ]] || builtin return 0
__ZZ_TITLE_INTEGRATION_INSTALLED=1

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

__zz_prompt_begin() {
  builtin local __zz_status=$?
  __zz_prompt_ready=0
  if [[ ${__zz_command_running-0} == 1 ]]; then
    builtin printf '\e]133;D;%s\a' "$__zz_status"
  fi
  __zz_command_running=0
  __zz_prompt_status=$__zz_status
  builtin return "$__zz_status"
}

__zz_prompt_debug() {
  builtin local __zz_status=$1 __zz_next=$2
  if [[ $__zz_next == __zz_prompt_begin ]]; then
    __zz_prompt_ready=0
  elif [[ ${__zz_prompt_ready-0} == 1 && ${BASH_SUBSHELL-0} == 0
          && ${#FUNCNAME[@]} == 1 && -z ${COMP_POINT-} && -z ${READLINE_POINT-} ]]; then
    __zz_prompt_ready=0
    __zz_command_running=1
    builtin printf '\e]133;C\a'
  fi
  builtin return "$__zz_status"
}

__zz_title_precmd() {
  __zz_write_working_directory
  __zz_write_title "${BASH##*/}"
  # DECSCUSR default: the prompt restores the configured cursor style instead of
  # imposing one, so a program that exits without resetting cannot keep its shape.
  builtin printf '\e[0 q'
  [[ $PS1 == *'\[\e]133;B\a\]' ]] || PS1+='\[\e]133;B\a\]'
  builtin printf '\e]133;A\a'
  __zz_prompt_ready=1
  builtin return "${__zz_prompt_status-0}"
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
  PROMPT_COMMAND=(__zz_prompt_begin "${PROMPT_COMMAND[@]}" __zz_title_precmd)
else
  PROMPT_COMMAND='__zz_prompt_begin'${PROMPT_COMMAND:+$'\n'"$PROMPT_COMMAND"}$'\n''__zz_title_precmd'
fi

__zz_prompt_ready=0
__zz_command_running=0
builtin eval "__zz_debug_trap=($(builtin trap -p DEBUG))"
builtin trap '__zz_prompt_debug "$?" "$BASH_COMMAND"'$'\n'"${__zz_debug_trap[2]-:}" DEBUG
builtin unset __zz_debug_trap
builtin unset ZZ_SHELL_INTEGRATION_ACTIVE
