#!/bin/sh
set -eu

depth_root="$HOME/source-depth"
rm -rf "$depth_root"
mkdir -p "$depth_root"

build_chain() {
    chain_dir="$1"
    chain_total="$2"
    chain_tag="$3"

    mkdir -p "$chain_dir"
    printf 'set-option -g @%sleaf yes\n' "$chain_tag" >"$chain_dir/leaf.conf"
    chain_level=1
    while [ "$chain_level" -le "$chain_total" ]; do
        {
            printf 'set-option -g @%sdepth %s\n' "$chain_tag" "$chain_level"
            if [ "$chain_level" -lt "$chain_total" ]; then
                printf 'source-file %s/f%s.conf\n' "$chain_dir" \
                    "$((chain_level + 1))"
            else
                if [ "$chain_tag" = r ]; then
                    printf 'source-file %s/leaf.conf ; set-option -g @source_line_depth_same yes\n' \
                        "$chain_dir"
                    printf 'set-option -g @source_line_depth_next yes\n'
                else
                    printf 'source-file %s/leaf.conf\n' "$chain_dir"
                fi
                printf 'source-file -q %s/leaf.conf\n' "$chain_dir"
            fi
            printf 'set-option -g @%safter%s yes\n' "$chain_tag" "$chain_level"
        } >"$chain_dir/f$chain_level.conf"
        chain_level=$((chain_level + 1))
    done
}

flatten() {
    sed "s|$depth_root/||g" "$1" | tr '\n' '~'
}

probe_chain() {
    probe_dir="$1"
    probe_total="$2"
    probe_tag="$3"
    probe_name="$4"

    build_chain "$probe_dir" "$probe_total" "$probe_tag"
    if tmux source-file "$probe_dir/f1.conf" \
        >"$depth_root/out" 2>"$depth_root/err"; then
        probe_rc=0
    else
        probe_rc=$?
    fi
    tmux set-environment -g "$probe_name" "rc=$probe_rc out=$(flatten "$depth_root/out") err=$(flatten "$depth_root/err") depth=$(tmux show-options -gqv "@${probe_tag}depth") last=$(tmux show-options -gqv "@${probe_tag}after$probe_total") leaf=$(tmux show-options -gqv "@${probe_tag}leaf")"
}

probe_chain "$depth_root/allowed" 49 a SOURCE_DEPTH_ALLOWED
probe_chain "$depth_root/refused" 50 r SOURCE_DEPTH_REFUSED

mkdir "$depth_root/matched-read-failure"
cat >"$depth_root/line-groups.conf" <<EOF
source-file missing-loud.conf ; set-option -g @source_line_loud_same yes
set-option -g @source_line_loud_next yes
source-file -q missing-quiet.conf ; set-option -g @source_line_quiet_same yes
set-option -g @source_line_quiet_next yes
kill-session -t =missing ; set-option -g @source_line_runtime_same yes
set-option -g @source_line_runtime_next yes
source-file '$depth_root/matched-read-failure' ; set-option -g @source_line_read_same yes
set-option -g @source_line_read_next yes
EOF

tmux source-file "$depth_root/line-groups.conf" \
    >"$depth_root/line-groups.out" 2>"$depth_root/line-groups.err" || :

marker() {
    marker_value=$(tmux show-options -gqv "$1")
    if [ "$marker_value" = yes ]; then
        printf yes
    else
        printf no
    fi
}

tmux set-environment -g SOURCE_LINE_GROUPS \
    "depth_same=$(marker @source_line_depth_same) depth_next=$(marker @source_line_depth_next) loud_same=$(marker @source_line_loud_same) loud_next=$(marker @source_line_loud_next) quiet_same=$(marker @source_line_quiet_same) quiet_next=$(marker @source_line_quiet_next) runtime_same=$(marker @source_line_runtime_same) runtime_next=$(marker @source_line_runtime_next) read_same=$(marker @source_line_read_same) read_next=$(marker @source_line_read_next)"
exit 0
