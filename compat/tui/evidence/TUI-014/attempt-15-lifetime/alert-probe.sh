#!/usr/bin/env bash
set -eEuo pipefail
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^probe_side zz/,$d' compat/attached-client.sh)"
probe_side zz
probe_side tmux
PS4='+${EPOCHREALTIME} '
set -x
probe_alert_message_lifecycle zz
set +x
printf 'candidate alert lifecycle passed\n'
