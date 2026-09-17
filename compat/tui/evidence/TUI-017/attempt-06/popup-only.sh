set -euo pipefail
source <(sed '/^if \[ "$LIFECYCLE_ONLY" -eq 1 \]; then/,$d' compat/attached-client.sh)
probe_display_popup zz
probe_display_popup tmux
printf 'attached popup-only compatibility: PASS\n'
