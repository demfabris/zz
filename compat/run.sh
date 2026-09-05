#!/usr/bin/env bash
# Builds zz and the pinned tmux reference, runs the differential scenario
# corpus, writes compat/results/summary.md after the attached fixture passes,
# or checks that canonical summary for drift.
#
#   compat/run.sh
#   compat/run.sh windows panes
#   compat/run.sh --strict-geometry
#   compat/run.sh --check-summary | --strict-geometry --attached-client
#   ZZ_COMPAT_ZZ=path/to/zz compat/run.sh ...   (skip the build, use that binary)
#   compat/run.sh --delta origin/main..HEAD --commands split-window,new-window
#   compat/run.sh --delta origin/main..HEAD --list   (print selection, run nothing)
set -euo pipefail

COMPAT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd -- "$COMPAT_DIR/.." && pwd)"
SCENARIOS_DIR="$COMPAT_DIR/scenarios"
RESULTS_DIR="$COMPAT_DIR/results"
FETCH_TMUX="$COMPAT_DIR/fetch-tmux.sh"
FETCH_CORPUS="$COMPAT_DIR/fetch-corpus.sh"
DIFF_SCENARIO="$COMPAT_DIR/diff-scenario.sh"
ATTACHED_CLIENT_FIXTURE="$COMPAT_DIR/attached-client.sh"
TMUX_ORACLE="$COMPAT_DIR/tmux-oracle.py"
TMUX_TRACKER="$COMPAT_DIR/tmux-tracker.py"

STRICT_GEOMETRY=0
CHECK_SUMMARY=0
ATTACHED_CLIENT=0
DELTA_RANGE=""
DELTA_COMMANDS=""
LIST_ONLY=0
requested=()

while [ "$#" -gt 0 ]; do
  case "$1" in
  --strict-geometry)
    STRICT_GEOMETRY=1
    shift
    ;;
  --delta)
    [ "$#" -ge 2 ] || { echo "run.sh: --delta needs a git range" >&2; exit 2; }
    DELTA_RANGE="$2"
    shift 2
    ;;
  --commands)
    [ "$#" -ge 2 ] || { echo "run.sh: --commands needs a comma-separated list" >&2; exit 2; }
    DELTA_COMMANDS="$2"
    shift 2
    ;;
  --list)
    LIST_ONLY=1
    shift
    ;;
  --check-summary)
    CHECK_SUMMARY=1
    shift
    ;;
  --attached-client)
    ATTACHED_CLIENT=1
    shift
    ;;
  -h | --help)
    sed -n '2,8p' "${BASH_SOURCE[0]}"
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
if [ -n "$DELTA_RANGE" ] && [ "${#requested[@]}" -eq 0 ]; then
  delta_list=""
  smoke_n=0
  changed_n=0
  matched_n=0
  shopt -s nullglob
  for f in "$SCENARIOS_DIR"/smoke/*.txt; do
    delta_list="$delta_list$(canonical_file "$f")"$'\n'
    smoke_n=$((smoke_n + 1))
  done
  shopt -u nullglob
  while IFS= read -r rel; do
    [ -f "$REPO_DIR/$rel" ] || continue
    case "$rel" in *.txt) ;; *) continue ;; esac
    delta_list="$delta_list$(canonical_file "$REPO_DIR/$rel")"$'\n'
    changed_n=$((changed_n + 1))
  done < <(git -C "$REPO_DIR" diff --name-only "$DELTA_RANGE" -- compat/scenarios/)
  if [ -n "$DELTA_COMMANDS" ]; then
    IFS=',' read -ra delta_cmds <<<"$DELTA_COMMANDS"
    for cmd in "${delta_cmds[@]}"; do
      cmd="$(printf '%s' "$cmd" | tr -d '[:space:]')"
      [ -n "$cmd" ] || continue
      while IFS= read -r f; do
        delta_list="$delta_list$(canonical_file "$f")"$'\n'
        matched_n=$((matched_n + 1))
      done < <(grep -rlF --include='*.txt' -- "$cmd" "$SCENARIOS_DIR" | sort)
    done
  fi
  while IFS= read -r f; do
    [ -n "$f" ] && scenarios+=("$f")
  done < <(printf '%s' "$delta_list" | sort -u)
  [ "$LIST_ONLY" -eq 1 ] || log "delta corpus for $DELTA_RANGE: ${#scenarios[@]} unique scenarios (smoke $smoke_n, changed $changed_n, command-matched $matched_n)"
elif [ "${#requested[@]}" -gt 0 ]; then
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

if [ "$LIST_ONLY" -eq 1 ]; then
  for s in "${scenarios[@]}"; do
    printf '%s\n' "${s#"$SCENARIOS_DIR/"}"
  done
  exit 0
fi

scenario_step_count() {
  awk '
    {
      line = $0
      sub(/\r$/, "", line)
      sub(/^[[:space:]]+/, "", line)
      sub(/[[:space:]]+$/, "", line)
      if (line == "" || substr(line, 1, 1) == "#") next
      if (line ~ /^(corpus:|shim:|launcher:|expect-warn:|stage:)/) next
      count++
    }
    END { print count + 0 }
  ' "$1"
}

scenario_corpus_mode() {
  awk '
    {
      line = $0
      sub(/\r$/, "", line)
      sub(/^[[:space:]]+/, "", line)
      sub(/[[:space:]]+$/, "", line)
      if (line !~ /^corpus:/) next
      count++
      sub(/^corpus:[[:space:]]*/, "", line)
      mode = line
    }
    END {
      if (count > 1 || (count == 1 && mode != "none" && mode != "required")) exit 2
      if (count == 1) print mode
    }
  ' "$1"
}

check_summary() {
  local summary_file="$1"
  local expected actual scenario scenario_relative scenario_name steps
  local expected_tuple expected_topo expected_geo expected_fmt expected_out expected_warn
  local topo_clean fmt_clean out_clean warn_clean
  local scenario_count total_steps attached_client_status
  local current_scenarios=()

  [ -f "$summary_file" ] || die "summary not found: $summary_file; run compat/run.sh"
  shopt -s nullglob
  current_scenarios=(
    "$SCENARIOS_DIR"/*.txt
    "$SCENARIOS_DIR"/known/*.txt
    "$SCENARIOS_DIR"/smoke/*.txt
  )
  shopt -u nullglob
  [ "${#current_scenarios[@]}" -gt 0 ] || die "no scenarios found under $SCENARIOS_DIR"
  expected="$(mktemp "$RESULTS_DIR/.expected-summary.XXXXXX")"
  actual="$(mktemp "$RESULTS_DIR/.actual-summary.XXXXXX")"

  for scenario in "${current_scenarios[@]}"; do
    case "$scenario" in
    "$SCENARIOS_DIR"/*) scenario_relative="${scenario#"$SCENARIOS_DIR"/}" ;;
    *) scenario_relative="$(basename -- "$scenario")" ;;
    esac
    scenario_name="${scenario_relative%.txt}"
    steps="$(scenario_step_count "$scenario")"
    expected_tuple="0 0 0 0 0"
    case "$scenario_relative" in
    known/*)
      if ! expected_tuple="$(python3 "$TMUX_TRACKER" known-tuple "$scenario_relative")"; then
        rm -f -- "$expected" "$actual"
        die "known scenario has no registered divergence tuple: $scenario_relative"
      fi
      ;;
    esac
    read -r expected_topo expected_geo expected_fmt expected_out expected_warn <<<"$expected_tuple"
    topo_clean="yes"
    fmt_clean="yes"
    out_clean="yes"
    warn_clean="yes"
    [ "$expected_topo" -eq 0 ] || topo_clean="no"
    [ "$expected_fmt" -eq 0 ] || fmt_clean="no"
    [ "$expected_out" -eq 0 ] || out_clean="no"
    [ "$expected_warn" -eq 0 ] || warn_clean="no"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$scenario_name" "$steps" "$topo_clean" "$expected_geo" "$fmt_clean" \
      "$out_clean" "$warn_clean" >>"$expected"
  done
  LC_ALL=C sort -o "$expected" "$expected"

  if ! awk -F '|' '
    function trim(value) {
      sub(/^[[:space:]]+/, "", value)
      sub(/[[:space:]]+$/, "", value)
      return value
    }
    /^\|/ {
      scenario = trim($2)
      steps = trim($3)
      topo = trim($4)
      geo = trim($5)
      fmt = trim($6)
      out = trim($7)
      warn = trim($8)
      if (scenario == "Scenario" || scenario == "---") next
      if (NF != 9 || scenario == "" || steps !~ /^[0-9]+$/ ||
          (topo != "yes" && topo != "no") || geo !~ /^[0-9]+$/ ||
          (fmt != "yes" && fmt != "no") ||
          (out != "yes" && out != "no") ||
          (warn != "yes" && warn != "no")) {
        malformed = 1
        next
      }
      print scenario "\t" steps "\t" topo "\t" geo "\t" fmt "\t" out "\t" warn
    }
    END { if (malformed) exit 2 }
  ' "$summary_file" >"$actual"; then
    rm -f -- "$expected" "$actual"
    die "malformed scenario row in $summary_file"
  fi
  LC_ALL=C sort -o "$actual" "$actual"

  if ! diff -u --label current-scenarios --label persisted-summary "$expected" "$actual"; then
    rm -f -- "$expected" "$actual"
    die "scenario inventory or result status drifted from $summary_file; run compat/run.sh"
  fi

  scenario_count="$(wc -l <"$expected" | tr -d '[:space:]')"
  total_steps="$(awk '{ total += $2 } END { print total + 0 }' "$expected")"
  attached_client_status="$(awk -F '`' '/^Status: `/{ print $2 }' "$summary_file")"
  if [ -z "$attached_client_status" ]; then
    rm -f -- "$expected" "$actual"
    die "attached-client status missing from $summary_file; run compat/run.sh"
  fi
  if [ "$attached_client_status" != "PASS" ]; then
    rm -f -- "$expected" "$actual"
    die "attached-client status is $attached_client_status in $summary_file; rerun with --attached-client"
  fi
  attached_client_commit="$(awk -F '`' '/^Recorded at: `/{ print $2 }' "$summary_file")"
  if [ -z "$attached_client_commit" ]; then
    rm -f -- "$expected" "$actual"
    die "attached-client PASS in $summary_file carries no commit stamp; a PASS that names no tree proves nothing, rerun compat/run.sh --attached-client"
  fi
  case "$attached_client_commit" in
  *-dirty)
    rm -f -- "$expected" "$actual"
    die "attached-client PASS in $summary_file was recorded on a dirty tree ($attached_client_commit); rerun compat/run.sh --attached-client on a clean checkout"
    ;;
  esac
  if ! git -C "$REPO_DIR" cat-file -e "$attached_client_commit^{commit}" 2>/dev/null; then
    rm -f -- "$expected" "$actual"
    die "attached-client PASS in $summary_file names commit $attached_client_commit, which this checkout does not have"
  fi
  if ! git -C "$REPO_DIR" merge-base --is-ancestor "$attached_client_commit" HEAD; then
    rm -f -- "$expected" "$actual"
    die "attached-client PASS in $summary_file was recorded at $attached_client_commit, which is not an ancestor of HEAD; rerun compat/run.sh --attached-client"
  fi
  if [ -n "$(git -C "$REPO_DIR" diff --name-only "$attached_client_commit" HEAD -- compat/attached-client.sh crates/)" ]; then
    rm -f -- "$expected" "$actual"
    die "attached-client PASS in $summary_file predates changes to the fixture or the crates since $attached_client_commit; rerun compat/run.sh --attached-client"
  fi
  rm -f -- "$expected" "$actual"
  printf 'summary current: %s scenarios, %s steps; attached-client %s at %s\n' \
    "$scenario_count" "$total_steps" "$attached_client_status" "$attached_client_commit"
}

if [ "$CHECK_SUMMARY" -eq 1 ]; then
  [ "${#requested[@]}" -eq 0 ] || die "--check-summary does not accept scenario names"
  [ "$ATTACHED_CLIENT" -eq 0 ] || die "--check-summary cannot be combined with --attached-client"
  check_summary "$RESULTS_DIR/summary.md"
  exit 0
fi

if [ "$ATTACHED_CLIENT" -eq 1 ]; then
  [ -x "$ATTACHED_CLIENT_FIXTURE" ] || die "attached-client fixture is not executable: $ATTACHED_CLIENT_FIXTURE"
fi

needs_corpus=0
for scenario in "${scenarios[@]}"; do
  case "$scenario" in
  "$SCENARIOS_DIR"/*) scenario_relative="${scenario#"$SCENARIOS_DIR"/}" ;;
  *) scenario_relative="$(basename -- "$scenario")" ;;
  esac
  if ! corpus_mode="$(scenario_corpus_mode "$scenario")"; then
    die "scenario has invalid corpus metadata: $scenario_relative"
  fi
  case "$scenario_relative" in
  smoke/*)
    [ -n "$corpus_mode" ] || die "smoke scenario must declare corpus: none or corpus: required: $scenario_relative"
    ;;
  *)
    [ -z "$corpus_mode" ] || die "corpus metadata is only valid for smoke scenarios: $scenario_relative"
    ;;
  esac
  if [ "$corpus_mode" = "required" ]; then
    needs_corpus=1
  fi
done

log "checking pinned tmux"
TMUX_BIN="$("$FETCH_TMUX")"
[ -x "$TMUX_BIN" ] || die "fetch-tmux.sh did not return an executable"

log "checking tmux oracle and compatibility tracker"
python3 "$TMUX_ORACLE" --check --tmux "$TMUX_BIN"
python3 "$TMUX_TRACKER" check

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
      warn "$corpus_skip_reason; plugin-dependent scenarios will be reported as SKIP"
    else
      die "fetch-corpus.sh failed with exit $corpus_rc"
    fi
  fi
fi

if [ -n "${ZZ_COMPAT_ZZ:-}" ]; then
  ZZ_BIN="$ZZ_COMPAT_ZZ"
  [ -x "$ZZ_BIN" ] || die "ZZ_COMPAT_ZZ is not an executable: $ZZ_BIN"
  [ -x "$(dirname -- "$ZZ_BIN")/zz_cli" ] ||
    warn "no zz_cli beside $ZZ_BIN; launcher scenarios will fail"
  log "using prebuilt zz: $ZZ_BIN"
else
  log "building zz"
  (
    cd "$REPO_DIR"
    cargo build -p zz
  )
  ZZ_BIN="$REPO_DIR/target/debug/zz"
  [ -x "$ZZ_BIN" ] || die "cargo build did not produce $ZZ_BIN"
fi

cd "$REPO_DIR"

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
skipped=0
for scenario in "${scenarios[@]}"; do
  case "$scenario" in
  "$SCENARIOS_DIR"/*) scenario_relative="${scenario#"$SCENARIOS_DIR"/}" ;;
  *) scenario_relative="$(basename -- "$scenario")" ;;
  esac
  scenario_name="${scenario_relative%.txt}"
  known_expected=""
  case "$scenario_relative" in
  known/*)
    if ! known_expected="$(python3 "$TMUX_TRACKER" known-tuple "$scenario_relative")"; then
      die "known scenario has no registered divergence tuple: $scenario_relative"
    fi
    ;;
  esac

  if ! corpus_mode="$(scenario_corpus_mode "$scenario")"; then
    die "scenario has invalid corpus metadata: $scenario_relative"
  fi

  if [ "$corpus_available" -eq 0 ] && [ "$corpus_mode" = "required" ]; then
    skipped=1
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
  fi

  log_file="$RESULTS_DIR/$scenario_name.log"
  rm -f -- "$log_file"
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
  topo_count=""
  fmt_count=""
  out_count=""
  warn_count=""
  if [ -n "$metadata" ]; then
    read -r -a metadata_tokens <<<"$metadata"
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

  if [ -n "$known_expected" ]; then
    actual_tuple="${topo_count:-?} ${geo_divergences:-?} ${fmt_count:-?} ${out_count:-?} ${warn_count:-?}"
    read -r expected_topo expected_geo expected_fmt expected_out expected_warn <<<"$known_expected"
    expected_rc=0
    if [ "$expected_topo" -gt 0 ] || [ "$expected_fmt" -gt 0 ] ||
      [ "$expected_out" -gt 0 ] || [ "$expected_warn" -gt 0 ] ||
      { [ "$STRICT_GEOMETRY" -eq 1 ] && [ "$expected_geo" -gt 0 ]; }; then
      expected_rc=1
    fi
    if [ "$actual_tuple" = "$known_expected" ] && [ "$scenario_rc" -eq "$expected_rc" ]; then
      warn "$scenario_name has its exact documented divergence ($known_expected)"
    else
      warn "$scenario_name changed from documented tuple ($known_expected) to ($actual_tuple), exit $scenario_rc"
      failed=1
    fi
  elif [ "$scenario_rc" -ne 0 ]; then
    warn "$scenario_name failed; see $log_file"
    if [ -f "$log_file" ]; then
      awk '/divergence$/{show=40} show>0{print "    " $0; show--}' "$log_file" >&2
    fi
    failed=1
  fi
done

attached_client_status="not run"
if [ "$ATTACHED_CLIENT" -eq 1 ]; then
  log "running attached-client"
  if "$ATTACHED_CLIENT_FIXTURE" "$ZZ_BIN" "$TMUX_BIN"; then
    attached_client_status="PASS"
    log "attached-client passed"
  else
    fixture_rc=$?
    attached_client_status="FAIL (exit $fixture_rc)"
    warn "attached-client failed with exit $fixture_rc"
    failed=1
  fi
fi

attached_client_commit="$(git -C "$REPO_DIR" rev-parse --short=12 HEAD 2>/dev/null || printf 'unknown')"
if [ -n "$(git -C "$REPO_DIR" status --porcelain --untracked-files=no -- compat/attached-client.sh crates/ 2>/dev/null)" ]; then
  attached_client_commit="$attached_client_commit-dirty"
fi
{
  printf '\n## Attached-client fixture\n\n'
  printf 'Status: `%s`\n' "$attached_client_status"
  if [ "$ATTACHED_CLIENT" -eq 1 ]; then
    printf 'Recorded at: `%s`\n' "$attached_client_commit"
  fi
} >>"$SUMMARY_TMP"

printf '\n'
cat "$SUMMARY_TMP"
printf '\n'

if [ "$failed" -ne 0 ]; then
  exit 1
fi

if [ "$skipped" -ne 0 ]; then
  rm -f -- "$SUMMARY_TMP"
  trap - EXIT
  log "run incomplete because scenarios were skipped; canonical summary left unchanged"
  exit 1
fi

if [ "${#requested[@]}" -eq 0 ] && [ "$ATTACHED_CLIENT" -eq 1 ]; then
  check_summary "$SUMMARY_TMP"
  mv "$SUMMARY_TMP" "$SUMMARY_FILE"
  trap - EXIT
  log "summary written to $SUMMARY_FILE"
elif [ "${#requested[@]}" -eq 0 ]; then
  rm -f -- "$SUMMARY_TMP"
  trap - EXIT
  log "full headless run complete; canonical summary left unchanged"
  log "rerun with --attached-client to persist the canonical summary"
else
  rm -f -- "$SUMMARY_TMP"
  trap - EXIT
  log "partial run complete; full summary left unchanged"
fi
