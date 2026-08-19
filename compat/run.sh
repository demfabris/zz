#!/usr/bin/env bash
# Builds zz and the pinned tmux reference, runs the differential scenario
# corpus, and writes compat/results/summary.md.
#
#   compat/run.sh
#   compat/run.sh windows panes
#   compat/run.sh --strict-geometry
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
SCENARIOS_DIR="$COMPAT_DIR/scenarios"
RESULTS_DIR="$COMPAT_DIR/results"
FETCH_TMUX="$COMPAT_DIR/fetch-tmux.sh"
FETCH_CORPUS="$COMPAT_DIR/fetch-corpus.sh"
DIFF_SCENARIO="$COMPAT_DIR/diff-scenario.sh"

STRICT_GEOMETRY=0
requested=()

while [ "$#" -gt 0 ]; do
  case "$1" in
  --strict-geometry)
    STRICT_GEOMETRY=1
    shift
    ;;
  -h | --help)
    sed -n '2,7p' "${BASH_SOURCE[0]}"
    exit 0
    ;;
  --)
    shift
    requested+=("$@")
    break
    ;;
  -*)
    echo "run.sh: unknown argument: $1" >&2
    exit 2
    ;;
  *)
    requested+=("$1")
    shift
    ;;
  esac
done

log() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarn:\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31merror:\033[0m %s\n' "$*" >&2
  exit 1
}

canonical_file() {
  local file="$1"
  printf '%s/%s\n' "$(cd -- "$(dirname -- "$file")" && pwd)" "$(basename -- "$file")"
}

resolve_scenario() {
  local name="$1"
  local candidate

  for candidate in \
    "$name" \
    "$SCENARIOS_DIR/$name" \
    "$SCENARIOS_DIR/$name.txt" \
    "$SCENARIOS_DIR/known/$name" \
    "$SCENARIOS_DIR/known/$name.txt" \
    "$SCENARIOS_DIR/smoke/$name" \
    "$SCENARIOS_DIR/smoke/$name.txt"; do
    if [ -f "$candidate" ]; then
      canonical_file "$candidate"
      return 0
    fi
  done
  return 1
}

scenarios=()
if [ "${#requested[@]}" -gt 0 ]; then
  for name in "${requested[@]}"; do
    scenario="$(resolve_scenario "$name" || true)"
    [ -n "$scenario" ] || die "scenario not found: $name"
    scenarios+=("$scenario")
  done
else
  shopt -s nullglob
  scenarios=(
    "$SCENARIOS_DIR"/*.txt
    "$SCENARIOS_DIR"/known/*.txt
    "$SCENARIOS_DIR"/smoke/*.txt
  )
  shopt -u nullglob
fi
[ "${#scenarios[@]}" -gt 0 ] || die "no scenarios found under $SCENARIOS_DIR"

needs_corpus=0
for scenario in "${scenarios[@]}"; do
  case "$scenario" in
  "$SCENARIOS_DIR/smoke/"*) needs_corpus=1 ;;
  esac
done

corpus_available=1
corpus_skip_reason=""
if [ "$needs_corpus" -eq 1 ]; then
  log "checking pinned plugin corpus"
  if corpus_dir="$("$FETCH_CORPUS")"; then
    export ZZ_COMPAT_CORPUS="$corpus_dir"
  else
    corpus_rc=$?
    if [ "$corpus_rc" -eq 3 ]; then
      corpus_available=0
      corpus_skip_reason="plugin corpus unavailable"
      warn "$corpus_skip_reason; smoke scenarios will be reported as SKIP"
    else
      die "fetch-corpus.sh failed with exit $corpus_rc"
    fi
  fi
fi

log "building zz"
(
  cd "$REPO_DIR"
  cargo build -p zz
)
ZZ_BIN="$REPO_DIR/target/debug/zz"
[ -x "$ZZ_BIN" ] || die "cargo build did not produce $ZZ_BIN"

log "checking pinned tmux"
TMUX_BIN="$("$FETCH_TMUX")"
[ -x "$TMUX_BIN" ] || die "fetch-tmux.sh did not return an executable"

mkdir -p "$RESULTS_DIR"
SUMMARY_FILE="$RESULTS_DIR/summary.md"
SUMMARY_TMP="$RESULTS_DIR/.summary.$$"

cleanup_summary() {
  rm -f -- "$SUMMARY_TMP"
}
trap cleanup_summary EXIT

{
  printf '# tmux compatibility summary\n\n'
  printf '| Scenario | Steps | TOPO clean? | GEO divergences | FMT clean? | OUT clean? | WARN clean? |\n'
  printf '| --- | ---: | :---: | ---: | :---: | :---: | :---: |\n'
} >"$SUMMARY_TMP"

failed=0
for scenario in "${scenarios[@]}"; do
  case "$scenario" in
  "$SCENARIOS_DIR"/*) scenario_relative="${scenario#"$SCENARIOS_DIR"/}" ;;
  *) scenario_relative="$(basename -- "$scenario")" ;;
  esac
  scenario_name="${scenario_relative%.txt}"

  if [ "$corpus_available" -eq 0 ]; then
    case "$scenario_relative" in
    smoke/*)
      log "SKIP $scenario_name ($corpus_skip_reason)"
      mkdir -p "$(dirname -- "$RESULTS_DIR/$scenario_name.log")"
      {
        printf '# Scenario: %s\n' "$scenario_name"
        printf 'SKIP: %s\n' "$corpus_skip_reason"
        printf 'SUMMARY status=skip steps=0 topo_divergences=0 geo_divergences=0 fmt_divergences=0 out_divergences=0 warn_divergences=0\n'
      } >"$RESULTS_DIR/$scenario_name.log"
      printf '| %s | 0 | SKIP | 0 | SKIP | SKIP | SKIP: corpus unavailable |\n' \
        "$scenario_name" >>"$SUMMARY_TMP"
      continue
      ;;
    esac
  fi

  log "running $scenario_name"

  diff_args=()
  if [ "$STRICT_GEOMETRY" -eq 1 ]; then
    diff_args+=(--strict-geometry)
  fi
  diff_args+=("$scenario" "$ZZ_BIN" "$TMUX_BIN")

  if "$DIFF_SCENARIO" "${diff_args[@]}"; then
    scenario_rc=0
  else
    scenario_rc=$?
  fi

  log_file="$RESULTS_DIR/$scenario_name.log"
  metadata=""
  if [ -f "$log_file" ]; then
    metadata="$(awk '/^SUMMARY / { value = $0 } END { print value }' "$log_file")"
  fi

  steps="?"
  topo_clean="?"
  geo_divergences="?"
  fmt_clean="?"
  out_clean="?"
  warn_clean="n/a"
  if [ -n "$metadata" ]; then
    read -r -a metadata_tokens <<<"$metadata"
    topo_count=""
    fmt_count=""
    out_count=""
    warn_count=""
    for token in "${metadata_tokens[@]}"; do
      case "$token" in
      steps=*) steps="${token#steps=}" ;;
      topo_divergences=*) topo_count="${token#topo_divergences=}" ;;
      geo_divergences=*) geo_divergences="${token#geo_divergences=}" ;;
      fmt_divergences=*) fmt_count="${token#fmt_divergences=}" ;;
      out_divergences=*) out_count="${token#out_divergences=}" ;;
      warn_divergences=*) warn_count="${token#warn_divergences=}" ;;
      esac
    done
    if [ "$topo_count" = "0" ]; then
      topo_clean="yes"
    elif [ -n "$topo_count" ]; then
      topo_clean="no"
    fi
    if [ "$fmt_count" = "0" ]; then
      fmt_clean="yes"
    elif [ -n "$fmt_count" ]; then
      fmt_clean="no"
    fi
    if [ "$out_count" = "0" ]; then
      out_clean="yes"
    elif [ -n "$out_count" ]; then
      out_clean="no"
    fi
    if [ "$warn_count" = "0" ]; then
      warn_clean="yes"
    elif [ -n "$warn_count" ]; then
      warn_clean="no"
    fi
  fi

  printf '| %s | %s | %s | %s | %s | %s | %s |\n' \
    "$scenario_name" "$steps" "$topo_clean" "$geo_divergences" "$fmt_clean" \
    "$out_clean" "$warn_clean" >>"$SUMMARY_TMP"

  expected=0
  case "$scenario_relative" in
  known/*) expected=1 ;;
  esac

  if [ "$scenario_rc" -eq 1 ] && [ "$expected" -eq 1 ] && [ "$fmt_count" = "0" ] &&
    [ "$out_count" = "0" ] && [ "${warn_count:-0}" = "0" ]; then
    warn "$scenario_name has its expected documented divergence"
  elif [ "$scenario_rc" -ne 0 ]; then
    warn "$scenario_name failed; see $log_file"
    if [ -f "$log_file" ]; then
      awk '/divergence$/{show=40} show>0{print "    " $0; show--}' "$log_file" >&2
    fi
    failed=1
  fi
done

mv "$SUMMARY_TMP" "$SUMMARY_FILE"
trap - EXIT

printf '\n'
cat "$SUMMARY_FILE"
printf '\n'
log "summary written to $SUMMARY_FILE"

if [ "$failed" -ne 0 ]; then
  exit 1
fi
