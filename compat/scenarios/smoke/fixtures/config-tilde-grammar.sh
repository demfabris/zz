#!/bin/sh
set -eu

export LC_ALL=C

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    side=zz
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    side=tmux
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

work="$HOME/config-tilde-grammar-$side"
mkdir -p "$work"

environment_value() {
    main_client show-environment -g "$1" 2>/dev/null | sed "s/^$1=//" || :
}

current_user=$(id -un)
passwd_home=$(python3 -c 'import os, pwd; print(pwd.getpwuid(os.getuid()).pw_dir)')
missing_user=zz_config_tilde_missing_298374
main_config="$work/main.conf"
empty_config="$work/empty.conf"
unset_config="$work/unset.conf"
parse_only_config="$work/parse-only.conf"
invalid_config="$work/invalid.conf"
parse_only_out="$work/parse-only.out"
invalid_out="$work/invalid.out"
invalid_err="$work/invalid.err"

printf '%s\n' \
    "set-environment -g CONFIG_TILDE_SINGLE 'single'~/after" \
    'set-environment -g CONFIG_TILDE_DOUBLE "double"~/after' \
    "set-environment -g CONFIG_TILDE_EMPTY_SINGLE ''~/after" \
    'set-environment -g CONFIG_TILDE_EMPTY_DOUBLE ""~/after' \
    "set-environment -g CONFIG_TILDE_LITERAL_SINGLE prefix''~/after" \
    'set-environment -g CONFIG_TILDE_LITERAL_DOUBLE prefix""~/after' \
    "set-environment -g CONFIG_TILDE_NAMED ~$current_user/named" \
    "set-environment -g CONFIG_TILDE_LITERAL prefix~$missing_user/literal" \
    >"$main_config"
printf '%s\n' \
    'set-environment -g CONFIG_TILDE_EMPTY ~/empty' \
    >"$empty_config"
printf '%s\n' \
    'set-environment -g CONFIG_TILDE_UNSET ~/unset' \
    >"$unset_config"
printf '%s\n' \
    'HOME=/file/home' \
    'set-environment -g CONFIG_TILDE_PARSE_ONLY wrong' \
    'set-environment -g CONFIG_TILDE_PARSE_HOME ~/parse-only' \
    >"$parse_only_config"
printf '%s\n' \
    'set-environment -g CONFIG_TILDE_PARSE_ONLY wrong' \
    "display-message -p ~$missing_user/missing" \
    >"$invalid_config"

main_client set-environment -g HOME /server/home
main_status=0
main_client source-file "$main_config" >/dev/null 2>&1 || main_status=$?

main_client set-environment -g HOME ''
empty_status=0
main_client source-file "$empty_config" >/dev/null 2>&1 || empty_status=$?

main_client set-environment -gu HOME
unset_status=0
main_client source-file "$unset_config" >/dev/null 2>&1 || unset_status=$?

main_client set-environment -g HOME /server/home
main_client set-environment -g CONFIG_TILDE_PARSE_ONLY baseline
parse_only_status=0
main_client source-file -nv "$parse_only_config" >"$parse_only_out" 2>/dev/null || parse_only_status=$?

invalid_status=0
main_client source-file -n "$invalid_config" >"$invalid_out" 2>"$invalid_err" || invalid_status=$?

result=broken
if [ "$main_status" -eq 0 ] && \
    [ "$empty_status" -eq 0 ] && \
    [ "$unset_status" -eq 0 ] && \
    [ "$parse_only_status" -eq 0 ] && \
    [ "$invalid_status" -eq 1 ] && \
    [ "$(environment_value CONFIG_TILDE_SINGLE)" = single/server/home/after ] && \
    [ "$(environment_value CONFIG_TILDE_DOUBLE)" = double/server/home/after ] && \
    [ "$(environment_value CONFIG_TILDE_EMPTY_SINGLE)" = /server/home/after ] && \
    [ "$(environment_value CONFIG_TILDE_EMPTY_DOUBLE)" = /server/home/after ] && \
    [ "$(environment_value CONFIG_TILDE_LITERAL_SINGLE)" = 'prefix~/after' ] && \
    [ "$(environment_value CONFIG_TILDE_LITERAL_DOUBLE)" = 'prefix~/after' ] && \
    [ "$(environment_value CONFIG_TILDE_NAMED)" = "$passwd_home/named" ] && \
    [ "$(environment_value CONFIG_TILDE_LITERAL)" = "prefix~$missing_user/literal" ] && \
    [ "$(environment_value CONFIG_TILDE_EMPTY)" = "$passwd_home/empty" ] && \
    [ "$(environment_value CONFIG_TILDE_UNSET)" = "$passwd_home/unset" ] && \
    [ "$(environment_value CONFIG_TILDE_PARSE_ONLY)" = baseline ] && \
    [ -z "$(environment_value CONFIG_TILDE_PARSE_HOME)" ] && \
    [ "$(environment_value HOME)" = /server/home ] && \
    grep -Fq "$parse_only_config:3: set-environment -g CONFIG_TILDE_PARSE_HOME /server/home/parse-only" "$parse_only_out" && \
    grep -Fqx "$invalid_config:2: syntax error" "$invalid_out" && \
    [ ! -s "$invalid_err" ]; then
    result=clean:16
fi

main_client set-environment -g CONFIG_TILDE_GRAMMAR "$result"
