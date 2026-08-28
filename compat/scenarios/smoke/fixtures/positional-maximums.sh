#!/bin/sh
set -eu

if [ -n "${ZZ_SMOKE_ZZ_BIN:-}" ]; then
    main_client() {
        "$ZZ_SMOKE_ZZ_BIN" --socket "$ZZ_SMOKE_ZZ_SOCKET" "$@"
    }
else
    main_client() {
        "$ZZ_SMOKE_TMUX_BIN" -L "$ZZ_SMOKE_TMUX_LABEL" "$@"
    }
fi

failed=0
probe() {
    label="$1"
    canonical="$2"
    maximum="$3"
    shift 3
    output_file="$HOME/positional-maximum-$label.out"
    error_file="$HOME/positional-maximum-$label.err"
    expected_file="$HOME/positional-maximum-$label.expected"
    expected="command $canonical: too many arguments (need at most $maximum)"
    printf '%s\n' "$expected" >"$expected_file"
    set +e
    main_client "$@" >"$output_file" 2>"$error_file"
    status=$?
    set -e
    if [ "$status" -ne 1 ] || [ -s "$output_file" ] || ! cmp -s "$error_file" "$expected_file"; then
        failed=1
    fi
}

output_path="$HOME/positional-maximum-output"
printf 'save sentinel' >"$output_path"
main_client set-buffer -b source alpha
active_before="$(main_client display-message -p '#{pane_id}')"

probe_spelling() {
    spelling="$1"
    canonical="$2"
    maximum="$3"
    case "$maximum" in
        0) probe "$spelling" "$canonical" "$maximum" "$spelling" "$output_path" ;;
        1) probe "$spelling" "$canonical" "$maximum" "$spelling" "$output_path" extra ;;
        2) probe "$spelling" "$canonical" "$maximum" "$spelling" "$output_path" extra third ;;
        3) probe "$spelling" "$canonical" "$maximum" "$spelling" "$output_path" extra third fourth ;;
        *) exit 2 ;;
    esac
}

while read -r spelling canonical maximum; do
    probe_spelling "$spelling" "$canonical" "$maximum"
done <<'CASES'
break-pane break-pane 0
breakp break-pane 0
capture-pane capture-pane 0
capturep capture-pane 0
choose-buffer choose-buffer 1
choose-tree choose-tree 1
clear-history clear-history 0
clearhist clear-history 0
clear-prompt-history clear-prompt-history 0
clearphist clear-prompt-history 0
command-prompt command-prompt 1
confirm-before confirm-before 1
confirm confirm-before 1
copy-mode copy-mode 0
delete-buffer delete-buffer 0
deleteb delete-buffer 0
detach-client detach-client 0
detach detach-client 0
display-message display-message 1
display display-message 1
display-panes display-panes 1
displayp display-panes 1
find-window find-window 1
findw find-window 1
has-session has-session 0
has has-session 0
if-shell if-shell 3
if if-shell 3
join-pane join-pane 0
joinp join-pane 0
kill-pane kill-pane 0
killp kill-pane 0
kill-server kill-server 0
kill-session kill-session 0
kill-window kill-window 0
killw kill-window 0
last-pane last-pane 0
lastp last-pane 0
last-window last-window 0
last last-window 0
list-buffers list-buffers 0
lsb list-buffers 0
list-clients list-clients 0
lsc list-clients 0
list-commands list-commands 1
lscm list-commands 1
list-keys list-keys 1
lsk list-keys 1
list-panes list-panes 0
lsp list-panes 0
list-sessions list-sessions 0
ls list-sessions 0
list-windows list-windows 0
lsw list-windows 0
load-buffer load-buffer 1
loadb load-buffer 1
lock-client lock-client 0
lockc lock-client 0
lock-server lock-server 0
lock lock-server 0
lock-session lock-session 0
locks lock-session 0
move-pane move-pane 0
movep move-pane 0
move-window move-window 0
movew move-window 0
next-layout next-layout 0
nextl next-layout 0
next-window next-window 0
next next-window 0
paste-buffer paste-buffer 0
pasteb paste-buffer 0
pipe-pane pipe-pane 1
pipep pipe-pane 1
previous-layout previous-layout 0
prevl previous-layout 0
previous-window previous-window 0
prev previous-window 0
refresh-client refresh-client 1
refresh refresh-client 1
rename-session rename-session 1
rename rename-session 1
rename-window rename-window 1
renamew rename-window 1
resize-pane resize-pane 1
resizep resize-pane 1
resize-window resize-window 1
resizew resize-window 1
rotate-window rotate-window 0
rotatew rotate-window 0
save-buffer save-buffer 1
saveb save-buffer 1
select-layout select-layout 1
selectl select-layout 1
select-pane select-pane 0
selectp select-pane 0
select-window select-window 0
selectw select-window 0
send-prefix send-prefix 0
set-buffer set-buffer 1
setb set-buffer 1
set-environment set-environment 2
setenv set-environment 2
set-hook set-hook 2
set-option set-option 2
set set-option 2
set-window-option set-window-option 2
setw set-window-option 2
show-buffer show-buffer 0
showb show-buffer 0
show-environment show-environment 1
showenv show-environment 1
show-hooks show-hooks 1
show-messages show-messages 0
showmsgs show-messages 0
show-options show-options 1
show show-options 1
show-prompt-history show-prompt-history 0
showphist show-prompt-history 0
show-window-options show-window-options 1
showw show-window-options 1
start-server start-server 0
start start-server 0
swap-pane swap-pane 0
swapp swap-pane 0
swap-window swap-window 0
swapw swap-window 0
switch-client switch-client 0
switchc switch-client 0
unbind-key unbind-key 1
unbind unbind-key 1
wait-for wait-for 1
wait wait-for 1
CASES

active_after="$(main_client display-message -p '#{pane_id}')"
buffers="$(main_client list-buffers -F '#{buffer_name}=#{buffer_sample}')"
saved="$(cat "$output_path")"
if [ "$active_after" != "$active_before" ] || [ "$buffers" != 'source=alpha' ] ||
    [ "$saved" != 'save sentinel' ]; then
    failed=1
fi

if [ "$failed" -eq 0 ]; then
    main_client set-environment -g POSITIONAL_MAXIMUMS clean
fi
