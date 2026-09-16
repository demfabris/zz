#!/usr/bin/env bash
# Orchestrates the terminal IO-throughput benchmark: checks deps, builds
# fixtures, drives each terminal through bench/inner.sh, then summarises.
#
#   bench/run.sh                                   # every detected terminal
#   bench/run.sh --terminals zz,ghostty+tmux       #
#   bench/run.sh --fresh --runs 7                  # wipe results, 7 runs each
#   bench/run.sh --list                            # show what was detected
#
# Read bench/README.md before trusting any number this prints.
set -uo pipefail

BENCH_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$BENCH_DIR/.." && pwd)"
RESULTS_DIR="$BENCH_DIR/results"
FIXTURE_DIR="$BENCH_DIR/fixtures"
INNER="$BENCH_DIR/inner.sh"

# Short on purpose: macOS caps sun_path at ~104 bytes.
ZZ_SOCKET="${ZZ_BENCH_SOCKET:-/tmp/zz-bench.sock}"
TMUX_SOCKET_NAME="zzbench"

GHOSTTY_BIN="${GHOSTTY_BIN:-$(command -v ghostty || echo /Applications/Ghostty.app/Contents/MacOS/ghostty)}"
# Nightly, staged out of /Applications so it never touches the stable install:
#   gh release download tip -R ghostty-org/ghostty -p Ghostty.dmg
# then mount and copy Ghostty.app into bench/.cache/ghostty-tip/.
GHOSTTY_TIP_BIN="${GHOSTTY_TIP_BIN:-$BENCH_DIR/.cache/ghostty-tip/Ghostty.app/Contents/MacOS/ghostty}"

RUNS="${ZZ_BENCH_RUNS:-5}"
DOOM_SECONDS="${ZZ_BENCH_DOOM_SECONDS:-30}"
TESTS="${ZZ_BENCH_TESTS:-cat-ascii,cat-unicode,doom-fire}"
PER_TERMINAL_TIMEOUT="${ZZ_BENCH_TIMEOUT:-900}"
TERMINALS=""
FRESH=0
LIST_ONLY=0
SKIP_FIXTURES=0
MANUAL_ZZ=0

while [ $# -gt 0 ]; do
  case "$1" in
  --terminals)
    TERMINALS="$2"
    shift 2
    ;;
  --terminals=*)
    TERMINALS="${1#--terminals=}"
    shift
    ;;
  --runs)
    RUNS="$2"
    shift 2
    ;;
  --runs=*)
    RUNS="${1#--runs=}"
    shift
    ;;
  --doom-seconds)
    DOOM_SECONDS="$2"
    shift 2
    ;;
  --doom-seconds=*)
    DOOM_SECONDS="${1#--doom-seconds=}"
    shift
    ;;
  --tests)
    TESTS="$2"
    shift 2
    ;;
  --tests=*)
    TESTS="${1#--tests=}"
    shift
    ;;
  --timeout)
    PER_TERMINAL_TIMEOUT="$2"
    shift 2
    ;;
  --fresh)
    FRESH=1
    shift
    ;;
  --list)
    LIST_ONLY=1
    shift
    ;;
  --skip-fixtures)
    SKIP_FIXTURES=1
    shift
    ;;
  --manual-zz)
    MANUAL_ZZ=1
    shift
    ;;
  -h | --help)
    sed -n '2,10p' "${BASH_SOURCE[0]}"
    exit 0
    ;;
  *)
    echo "run.sh: unknown argument: $1" >&2
    exit 2
    ;;
  esac
done

log() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
  exit 1
}

# The release-optimised bundle only. A debug build measures the wrong thing.
find_zz() {
  local candidate
  for candidate in \
    "$REPO_DIR/dist/zz/zz.app/Contents/MacOS/zz" \
    "$REPO_DIR/dist/zz-profile/zz.app/Contents/MacOS/zz" \
    "$REPO_DIR/dist/zz/zz"; do
    if [ -x "$candidate" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

ZZ_BIN="$(find_zz || true)"

detect() {
  local found=()
  [ -n "$ZZ_BIN" ] && found+=(zz)
  if [ -x "$GHOSTTY_BIN" ]; then
    found+=(ghostty)
    command -v tmux >/dev/null 2>&1 && found+=(ghostty+tmux)
  fi
  [ -x "$GHOSTTY_TIP_BIN" ] && found+=(ghostty-tip)
  command -v kitty >/dev/null 2>&1 && found+=(kitty)
  command -v alacritty >/dev/null 2>&1 && found+=(alacritty)
  [ -x /Applications/kitty.app/Contents/MacOS/kitty ] &&
    ! command -v kitty >/dev/null 2>&1 && found+=(kitty)
  [ -x /Applications/Alacritty.app/Contents/MacOS/alacritty ] &&
    ! command -v alacritty >/dev/null 2>&1 && found+=(alacritty)
  [ "${#found[@]}" -eq 0 ] && return 0
  printf '%s\n' "${found[@]}" | paste -sd, -
}

DETECTED="$(detect)"

if [ "$LIST_ONLY" -eq 1 ]; then
  echo "detected terminals: ${DETECTED:-<none>}"
  echo "zz binary:          ${ZZ_BIN:-<not built>}"
  echo "ghostty binary:     $([ -x "$GHOSTTY_BIN" ] && echo "$GHOSTTY_BIN" || echo '<not found>')"
  echo "ghostty tip:        $([ -x "$GHOSTTY_TIP_BIN" ] && echo "$GHOSTTY_TIP_BIN" || echo '<not staged: see comment atop run.sh>')"
  echo "tmux:               $(command -v tmux || echo '<not found>')"
  echo "hyperfine:          $(command -v hyperfine || echo '<not found: falls back to /usr/bin/time>')"
  echo "jq:                 $(command -v jq || echo '<not found: needed for summarize.sh>')"
  exit 0
fi

[ -n "$TERMINALS" ] || TERMINALS="$DETECTED"
[ -n "$TERMINALS" ] || die "no terminals detected; see bench/run.sh --list"

command -v jq >/dev/null 2>&1 || warn "jq is missing: summarize.sh will not run"
command -v hyperfine >/dev/null 2>&1 ||
  warn "hyperfine is missing: falling back to a /usr/bin/time loop (brew install hyperfine)"

if [ "$SKIP_FIXTURES" -eq 0 ]; then
  log "checking fixtures"
  bash "$BENCH_DIR/gen-fixtures.sh" || die "gen-fixtures.sh failed"
else
  [ -f "$FIXTURE_DIR/150MB_ascii.txt" ] || die "--skip-fixtures but no fixtures exist"
fi

mkdir -p "$RESULTS_DIR"
if [ "$FRESH" -eq 1 ]; then
  log "clearing $RESULTS_DIR"
  rm -f "$RESULTS_DIR"/*.jsonl "$RESULTS_DIR"/*.done "$RESULTS_DIR"/*.log \
    "$RESULTS_DIR"/*.doom-tail.bin "$RESULTS_DIR"/summary.md
fi

INNER_ARGS="--runs=$RUNS --doom-seconds=$DOOM_SECONDS --tests=$TESTS"

cat <<BANNER

  ┌──────────────────────────────────────────────────────────────────────┐
  │  Benchmark session starting.                                         │
  │  Keep the machine on AC power, close heavy apps, and leave the       │
  │  launched terminal window FRONTMOST and unobscured: every one of     │
  │  these terminals throttles or skips painting when it is not visible. │
  │  Do not type into the window; it drives itself and closes when done. │
  └──────────────────────────────────────────────────────────────────────┘

  terminals : $TERMINALS
  tests     : $TESTS
  runs      : $RUNS  (doom-fire: ${DOOM_SECONDS}s)
  results   : $RESULTS_DIR/results.jsonl

BANNER

wait_for_done() {
  local label="$1" waited=0
  local marker="$RESULTS_DIR/$label.done"
  while [ ! -f "$marker" ]; do
    sleep 1
    waited=$((waited + 1))
    if [ $((waited % 15)) -eq 0 ]; then
      printf '    ...still running (%ss)\n' "$waited"
    fi
    if [ "$waited" -ge "$PER_TERMINAL_TIMEOUT" ]; then
      warn "$label did not finish within ${PER_TERMINAL_TIMEOUT}s"
      return 1
    fi
  done
  log "$label finished in ${waited}s"
  return 0
}

run_terminal() {
  local label="$1"
  shift
  rm -f "$RESULTS_DIR/$label.done"
  log "launching $label"
  "$@" >/dev/null 2>&1 &
  local launcher=$!
  wait_for_done "$label"
  local rc=$?
  kill "$launcher" 2>/dev/null
  wait "$launcher" 2>/dev/null
  return $rc
}

run_zz() {
  if [ -z "$ZZ_BIN" ]; then
    warn "no release zz bundle found under dist/"
    printf '      build one with:  cd %s && cargo xtask bundle-cef --release --output dist/zz\n' "$REPO_DIR"
    MANUAL_ZZ=1
  fi

  if [ "$MANUAL_ZZ" -eq 1 ]; then
    cat <<MANUAL

  ── zz: manual mode ─────────────────────────────────────────────────────
  Open a zz terminal pane yourself, make the window frontmost, and paste:

      bash $INNER zz $INNER_ARGS

  Waiting for it to finish (Ctrl-C to skip)...
MANUAL
    rm -f "$RESULTS_DIR/zz.done"
    wait_for_done zz
    return $?
  fi

  rm -f "$RESULTS_DIR/zz.done" "$ZZ_SOCKET"
  log "launching zz ($ZZ_BIN) on $ZZ_SOCKET"
  "$ZZ_BIN" --socket "$ZZ_SOCKET" >/dev/null 2>&1 &
  local gui=$!

  local waited=0
  while [ ! -S "$ZZ_SOCKET" ]; do
    sleep 0.5
    waited=$((waited + 1))
    if [ "$waited" -gt 120 ]; then
      warn "zz daemon never came up at $ZZ_SOCKET"
      kill "$gui" 2>/dev/null
      return 1
    fi
  done
  # The GUI still has to attach and lay out the first pane.
  sleep 3

  local pane
  pane="$("$ZZ_BIN" --socket "$ZZ_SOCKET" list-panes 2>/dev/null | head -1 | cut -d: -f1)"
  if [ -z "$pane" ]; then
    warn "zz: list-panes returned nothing; falling back to manual mode"
    kill "$gui" 2>/dev/null
    MANUAL_ZZ=1
    run_zz
    return $?
  fi
  log "zz: driving pane $pane"
  "$ZZ_BIN" --socket "$ZZ_SOCKET" send-keys -t "$pane" \
    "bash $INNER zz $INNER_ARGS" Enter >/dev/null

  wait_for_done zz
  local rc=$?
  "$ZZ_BIN" --socket "$ZZ_SOCKET" kill-server >/dev/null 2>&1
  sleep 1
  kill "$gui" 2>/dev/null
  wait "$gui" 2>/dev/null
  rm -f "$ZZ_SOCKET"
  return $rc
}

run_ghostty() {
  # shellcheck disable=SC2086 # INNER_ARGS is a deliberate word list
  run_terminal ghostty "$GHOSTTY_BIN" -e "$INNER" ghostty $INNER_ARGS
}

run_ghostty_tip() {
  # shellcheck disable=SC2086
  run_terminal ghostty-tip "$GHOSTTY_TIP_BIN" -e "$INNER" ghostty-tip $INNER_ARGS
}

run_ghostty_tmux() {
  # Dedicated server socket and -f /dev/null so no user tmux config leaks in.
  tmux -L "$TMUX_SOCKET_NAME" kill-server >/dev/null 2>&1
  run_terminal ghostty+tmux "$GHOSTTY_BIN" -e \
    tmux -L "$TMUX_SOCKET_NAME" -f /dev/null \
    new-session "bash $INNER ghostty+tmux $INNER_ARGS"
  local rc=$?
  tmux -L "$TMUX_SOCKET_NAME" kill-server >/dev/null 2>&1
  return $rc
}

run_simple() {
  local label="$1" bin="$2"
  # shellcheck disable=SC2086
  run_terminal "$label" "$bin" -e "$INNER" "$label" $INNER_ARGS
}

resolve_bin() {
  local name="$1"
  if command -v "$name" >/dev/null 2>&1; then
    command -v "$name"
    return 0
  fi
  local app="/Applications/${name}.app/Contents/MacOS/${name}"
  [ -x "$app" ] && printf '%s' "$app" && return 0
  app="/Applications/$(printf '%s' "${name:0:1}" | tr '[:lower:]' '[:upper:]')${name:1}.app/Contents/MacOS/${name}"
  [ -x "$app" ] && printf '%s' "$app" && return 0
  return 1
}

FAILED=()
IFS=',' read -r -a wanted <<<"$TERMINALS"
for terminal in "${wanted[@]}"; do
  case "$terminal" in
  "") continue ;;
  zz) run_zz || FAILED+=("$terminal") ;;
  ghostty)
    [ -x "$GHOSTTY_BIN" ] || {
      warn "ghostty not found at $GHOSTTY_BIN"
      FAILED+=("$terminal")
      continue
    }
    run_ghostty || FAILED+=("$terminal")
    ;;
  ghostty+tmux)
    [ -x "$GHOSTTY_BIN" ] || {
      warn "ghostty not found at $GHOSTTY_BIN"
      FAILED+=("$terminal")
      continue
    }
    command -v tmux >/dev/null 2>&1 || {
      warn "tmux not found"
      FAILED+=("$terminal")
      continue
    }
    run_ghostty_tmux || FAILED+=("$terminal")
    ;;
  ghostty-tip)
    [ -x "$GHOSTTY_TIP_BIN" ] || {
      warn "ghostty tip not staged at $GHOSTTY_TIP_BIN (see comment atop run.sh)"
      FAILED+=("$terminal")
      continue
    }
    run_ghostty_tip || FAILED+=("$terminal")
    ;;
  kitty | alacritty)
    bin="$(resolve_bin "$terminal" || true)"
    [ -n "$bin" ] || {
      warn "$terminal not installed; skipping"
      continue
    }
    run_simple "$terminal" "$bin" || FAILED+=("$terminal")
    ;;
  *)
    warn "unknown terminal '$terminal' (see bench/README.md to add one)"
    ;;
  esac
done

echo
if [ "${#FAILED[@]}" -gt 0 ]; then
  warn "these terminals did not complete: ${FAILED[*]}"
fi

if command -v jq >/dev/null 2>&1; then
  bash "$BENCH_DIR/summarize.sh" | tee "$RESULTS_DIR/summary.md"
  echo
  log "summary written to $RESULTS_DIR/summary.md"
else
  log "raw results in $RESULTS_DIR/results.jsonl (install jq for the summary)"
fi
