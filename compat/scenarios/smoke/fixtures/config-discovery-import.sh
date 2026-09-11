#!/bin/sh
set -eu
mode=${1:-default}
target="$XDG_CONFIG_HOME/zz/mux.conf"
for key in home xdg fallback home-xdg-order; do
    test -z "$(tmux show-options -gqv "@discovery-$key")"
done
cp "$target" "$HOME/mux-before-import"
tmux import-tmux-config > "$HOME/import-message"
grep -q 'commands copied' "$HOME/import-message"
grep -q "$HOME/.tmux.conf" "$HOME/import-message"
grep -q "$target" "$HOME/import-message"
head -n 1 "$target" | grep -q '^# zz-import-tmux-begin: '
sed '1,/^# zz-import-tmux-end: /d' "$target" > "$HOME/mux-after-import"
cmp "$HOME/mux-before-import" "$HOME/mux-after-import"
cp "$HOME/.tmux.conf" "$HOME/chosen donor.conf"
printf '\nclock-mode\n' >> "$HOME/chosen donor.conf"
tmux import-tmux-config "$HOME/chosen donor.conf" > "$HOME/reimport-message"
test "$(grep -c '^# zz-import-tmux-begin: ' "$target")" = 1
grep -q '^# zz-unsupported: clock-mode' "$target"
sed '1,/^# zz-import-tmux-end: /d' "$target" > "$HOME/mux-after-import"
cmp "$HOME/mux-before-import" "$HOME/mux-after-import"
if test "$mode" = explicit; then
    test -z "$(tmux show-options -gqv @discovery-home)"
    test "$(tmux show-options -gqv @discovery-order)" = explicit
    test "$(tmux show-options -gqv @discovery-layer)" = explicit
else
    test "$(tmux show-options -gqv @discovery-home)" = present
    test "$(tmux show-options -gqv @discovery-layer)" = mux-last
fi
tmux set-option -g @import-proof passed
