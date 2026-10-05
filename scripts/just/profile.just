import "settings.just"

set working-directory := "../.."

mod summary "profile-summary.just"

# Build a release-optimized macOS bundle with matching source-level dSYMs.
build platform:
    @if [[ "{{ platform }}" != "mac" ]]; then echo "profiling bundles currently support macOS only" >&2; exit 2; fi
    @if [[ "$(uname -s)" != "Darwin" ]]; then echo "just profile build mac requires macOS" >&2; exit 2; fi
    @version="$(zig version)"; if [[ "$version" != "{{ zig_version }}" ]]; then echo "Zig {{ zig_version }} is required, found $version" >&2; exit 2; fi; cargo xtask bundle-cef --profile profiling --output dist/zz-profile

# Profile GUI, daemon, or all-process CPU work in a fresh isolated macOS run.
cpu platform target="gui" duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "CPU capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh cpu "{{ target }}" "{{ duration }}"

memory platform duration="60s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Memory capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh memory all "{{ duration }}"

startup platform duration="8s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Startup capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh startup gui "{{ duration }}"

# Profile scheduling, waits, wakeups, and IPC in a fresh isolated macOS run.
system platform duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "System Trace capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh system all "{{ duration }}"

# Profile CEF and GPUI Metal work in a fresh isolated macOS run.
metal platform duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Metal capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh metal gui "{{ duration }}"

# Collect terminal row-cache diagnostics without treating logged timings as a clean CPU benchmark.
terminal platform duration="20s":
    @if [[ "{{ platform }}" != "mac" ]]; then echo "Terminal diagnostic capture currently supports macOS only" >&2; exit 2; fi
    @scripts/profile-macos.sh diagnostics gui "{{ duration }}"
