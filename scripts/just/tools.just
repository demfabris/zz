import "settings.just"

set working-directory := "../.."

# Asks a throwaway daemon built from this tree, never the running one, whose
# catalog text is whatever binary it started with.
skill:
    @cargo build -q -p zz --bin zz && unset TMUX && export ZZ_SOCKET=/tmp/zz-tools-skill.sock && rm -f "$ZZ_SOCKET" && target/debug/zz new-session -d -s tools-skill -c /tmp && skill="$(target/debug/zz tools --skill)"; rc=$?; target/debug/zz kill-server >/dev/null 2>&1; rm -f "$ZZ_SOCKET"; [ "$rc" -eq 0 ] && printf '%s\n' "$skill" > .agents/skills/zz-workspace/SKILL.md
