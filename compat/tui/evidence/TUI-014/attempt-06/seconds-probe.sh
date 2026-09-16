#!/usr/bin/env bash
set -eEuo pipefail
cd "$(dirname "$0")/../../../../.."
eval "$(sed -e 's|^COMPAT_DIR=.*|COMPAT_DIR="$PWD/compat"|' -e '/^if \[ "$SELF_CHECK" -eq 1 \]; then/,$d' compat/tui-client-commands.sh)"
capture_seconds_pair() {
  local before_zz before_tmux start now attempt poll
  for ((attempt = 0; attempt < 8; attempt++)); do
    before_zz="$(styled_screen_of zz)"
    before_tmux="$(styled_screen_of tmux)"
    start="$(date +%s)"
    for ((poll = 0; poll < 150; poll++)); do
      now="$(date +%s)"
      [ "$now" != "$start" ] && break
      sleep 0.01
    done
    [ "$now" != "$start" ] || continue
    sleep 0.2
    for ((poll = 0; poll < 100; poll++)); do
      zz_screen="$(styled_screen_of zz)"
      tmux_screen="$(styled_screen_of tmux)"
      zz_cursor="$(cursor_tuple zz)"
      tmux_cursor="$(cursor_tuple tmux)"
      [ "$(date +%s)" = "$now" ] || break
      if [ "$zz_screen" != "$before_zz" ] && [ "$tmux_screen" != "$before_tmux" ]; then
        printf 'clock capture %s: both faces redrew; screen and cursor pair stayed inside epoch second %s\n' "$CASE_LABEL" "$now"
        return 0
      fi
      sleep 0.01
    done
  done
  die "could not capture both seconds faces after redraw within one second"
}

attach_both_at 80 24
for style in 24-with-seconds 12-with-seconds; do
  set_window_on_both clock-mode-style "$style"
  printf 'SECONDS_SAMPLE_START %s %s\n' "$style" "$(date -Ins)"
  CASE_CLOCK_FACE=2
  case_run "clock-$style" same '' -- clock-mode -t PANE
  printf 'SECONDS_SAMPLE_END %s %s\n' "$style" "$(date -Ins)"
  restore_case "clock-$style-closed"
done
printf 'seconds probe: %s assertions including both seconds faces, %s failures\n' "$CHECKS" "$FAILURES"
[ "$FAILURES" -eq 0 ]
