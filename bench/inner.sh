#!/usr/bin/env bash
# Runs INSIDE the terminal under test. Everything it measures is real output
# that the host terminal had to parse and paint.
#
#   bench/inner.sh <label> [--smoke] [--tests=cat-ascii,cat-unicode,doom-fire]
#
# Appends one JSON object per test to bench/results/results.jsonl and drops
# bench/results/<label>.done when finished, which is what run.sh polls on.
#
# Deliberately plain bash: it has to survive tmux, a zz pane, and a pipe.
set -uo pipefail

BENCH_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
FIXTURE_DIR="$BENCH_DIR/fixtures"
RESULTS_DIR="${ZZ_BENCH_RESULTS:-$BENCH_DIR/results}"
DOOM_BIN="$BENCH_DIR/.cache/DOOM-fire-zig/zig-out/bin/DOOM-fire"

RUNS="${ZZ_BENCH_RUNS:-5}"
WARMUP="${ZZ_BENCH_WARMUP:-1}"
DOOM_SECONDS="${ZZ_BENCH_DOOM_SECONDS:-30}"
TESTS="${ZZ_BENCH_TESTS:-cat-ascii,cat-unicode,doom-fire}"

# Pin the producer: aliases never reach scripts but PATH does (GNU coreutils
# shadows cat with gcat), and the upstream numbers were made with stock
# /bin/cat. NEVER benchmark by typing `cat` in an interactive shell: an alias
# like cat='bat' measures the alias, not the terminal.
CAT_BIN=/bin/cat
[ -x "$CAT_BIN" ] || CAT_BIN="$(command -v cat)"

LABEL=""
for arg in "$@"; do
	case "$arg" in
	--smoke)
		RUNS=1
		WARMUP=0
		DOOM_SECONDS=3
		;;
	--tests=*) TESTS="${arg#--tests=}" ;;
	--runs=*) RUNS="${arg#--runs=}" ;;
	--doom-seconds=*) DOOM_SECONDS="${arg#--doom-seconds=}" ;;
	-h | --help)
		sed -n '2,10p' "${BASH_SOURCE[0]}"
		exit 0
		;;
	-*)
		echo "inner.sh: unknown flag: $arg" >&2
		exit 2
		;;
	*) LABEL="$arg" ;;
	esac
done

if [ -z "$LABEL" ]; then
	echo "usage: inner.sh <label> [--smoke] [--tests a,b,c]" >&2
	exit 2
fi

mkdir -p "$RESULTS_DIR"
JSONL="$RESULTS_DIR/results.jsonl"
DONE_MARKER="$RESULTS_DIR/$LABEL.done"
LOG="$RESULTS_DIR/$LABEL.log"
rm -f "$DONE_MARKER"
: >"$LOG"

note() { printf '[%s] %s\n' "$LABEL" "$*" >>"$LOG"; }

# Never `tput cols` here: inside a command substitution tput's stdout is a
# pipe, its TIOCGWINSZ fails, and it silently returns terminfo's static 80x24
# instead of the real grid. stty asks a real tty.
read_grid() {
	local size=""
	[ -r /dev/tty ] && size="$(stty size </dev/tty 2>/dev/null)"
	[ -n "$size" ] || size="$(stty size 2>/dev/null)"
	if [ -n "$size" ]; then
		LINES_="${size%% *}"
		COLS="${size##* }"
	else
		COLS=0
		LINES_=0
	fi
}

if [ -t 1 ]; then
	IS_TTY=true
	# A terminal resizes the pty a beat after the child starts, so an immediate
	# read returns the pre-resize default (80x24 out of Ghostty), mislabels every
	# row and misjudges whether DOOM-fire fits. Wait that out, then require two
	# agreeing reads.
	sleep "${ZZ_BENCH_SETTLE:-2}"
	read_grid
	settle_prev="${COLS}x${LINES_}"
	for _ in $(seq 1 20); do
		sleep 0.25
		read_grid
		[ "${COLS}x${LINES_}" = "$settle_prev" ] && break
		settle_prev="${COLS}x${LINES_}"
	done
else
	IS_TTY=false
	COLS="${COLUMNS:-120}"
	LINES_="${LINES:-30}"
fi
[ "${COLS:-0}" -gt 0 ] 2>/dev/null || COLS=120
[ "${LINES_:-0}" -gt 0 ] 2>/dev/null || LINES_=30

HW_MODEL="$(sysctl -n hw.model 2>/dev/null || uname -m)"
OS_VERSION="$(sw_vers -productVersion 2>/dev/null || uname -r)"

# hyperfine is only usable with jq: per-run times come out of its JSON export.
if command -v hyperfine >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
	TIMER_TOOL="hyperfine"
elif [ -x /usr/bin/time ]; then
	TIMER_TOOL="time-loop"
else
	echo "inner.sh: neither hyperfine+jq nor /usr/bin/time is available" >&2
	exit 1
fi

TIMEOUT_BIN=""
for candidate in timeout gtimeout; do
	if command -v "$candidate" >/dev/null 2>&1; then
		TIMEOUT_BIN="$(command -v "$candidate")"
		break
	fi
done

file_size() { stat -c %s "$1" 2>/dev/null || stat -f %z "$1"; }

sha12() {
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum "$1" | cut -c1-12
	else
		shasum -a 256 "$1" | cut -c1-12
	fi
}

json_escape() {
	printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/	/\\t/g'
}

# median <numbers...>
median() {
	printf '%s\n' "$@" | sort -g | awk '
		{ v[NR] = $1 }
		END {
			if (NR == 0) { print "null"; exit }
			if (NR % 2) printf "%.3f\n", v[(NR + 1) / 2]
			else printf "%.3f\n", (v[NR / 2] + v[NR / 2 + 1]) / 2
		}'
}

# emit_result <test> <extra-json-fields...>
# A single printf so the O_APPEND write stays atomic across concurrent panes.
emit_result() {
	local test_name="$1"
	shift
	printf '{"label":"%s","test":"%s","cols":%s,"lines":%s,"tty":%s,"term":"%s","term_program":"%s","term_program_version":"%s","hw_model":"%s","macos":"%s","timestamp":"%s","runs":%s,%s}\n' \
		"$(json_escape "$LABEL")" \
		"$(json_escape "$test_name")" \
		"$COLS" "$LINES_" "$IS_TTY" \
		"$(json_escape "${TERM:-}")" \
		"$(json_escape "${TERM_PROGRAM:-}")" \
		"$(json_escape "${TERM_PROGRAM_VERSION:-}")" \
		"$(json_escape "$HW_MODEL")" \
		"$(json_escape "$OS_VERSION")" \
		"$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
		"$RUNS" \
		"$*" >>"$JSONL"
}

emit_skip() {
	local test_name="$1" reason="$2"
	emit_result "$test_name" "\"status\":\"skipped\",\"reason\":\"$(json_escape "$reason")\""
	note "skipped $test_name: $reason"
}

# Both timers fill RUN_TIMES (milliseconds) rather than printing it. The
# measured command's stdout MUST stay attached to the terminal, so neither
# timer may be called inside a command substitution; that would swallow all
# 150 MiB into a pipe and measure nothing at all.
RUN_TIMES=()

time_with_hyperfine() {
	local fixture="$1" json="$RESULTS_DIR/.hyperfine.$$.json" list
	rm -f "$json"
	# --show-output gives the child its own stdout instead of /dev/null (the
	# default), which is the entire point here; it excludes --style.
	# --shell=none execs cat directly so no shell startup lands in the sample.
	if ! hyperfine --shell=none --show-output \
		--warmup "$WARMUP" --runs "$RUNS" \
		--export-json "$json" \
		"'$CAT_BIN' '$fixture'" 2>>"$LOG"; then
		rm -f "$json"
		return 1
	fi
	list="$(jq -r '.results[0].times | map(. * 1000) | join(" ")' "$json" 2>>"$LOG")"
	rm -f "$json"
	[ -n "$list" ] || return 1
	read -r -a RUN_TIMES <<<"$list"
	[ "${#RUN_TIMES[@]}" -gt 0 ]
}

time_with_loop() {
	local fixture="$1" tmp="$RESULTS_DIR/.time.$$" i out
	RUN_TIMES=()
	for ((i = 0; i < WARMUP; i++)); do "$CAT_BIN" "$fixture"; done
	for ((i = 0; i < RUNS; i++)); do
		# stdout stays on the terminal; only /usr/bin/time's report is captured.
		{ /usr/bin/time -p "$CAT_BIN" "$fixture"; } 2>"$tmp"
		out="$(awk '/^real/ { printf "%.3f", $2 * 1000 }' "$tmp")"
		RUN_TIMES+=("${out:-0}")
	done
	rm -f "$tmp"
	[ "${#RUN_TIMES[@]}" -gt 0 ]
}

run_cat_test() {
	local test_name="$1" fixture="$2"
	if [ ! -f "$fixture" ]; then
		emit_skip "$test_name" "missing fixture $(basename "$fixture"): run bench/gen-fixtures.sh"
		return
	fi

	local bytes sha med mbps times_json
	bytes="$(file_size "$fixture")"
	sha="$(sha12 "$fixture")"

	note "$test_name: $TIMER_TOOL, $RUNS runs, $WARMUP warmup"
	RUN_TIMES=()
	if [ "$TIMER_TOOL" = hyperfine ]; then
		if ! time_with_hyperfine "$fixture"; then
			note "hyperfine failed, falling back to the time loop"
			TIMER_TOOL="time-loop"
			time_with_loop "$fixture"
		fi
	else
		time_with_loop "$fixture"
	fi

	if [ "${#RUN_TIMES[@]}" -eq 0 ]; then
		emit_skip "$test_name" "no timings were produced (see $(basename "$LOG"))"
		return
	fi

	med="$(median "${RUN_TIMES[@]}")"
	mbps="$(awk -v b="$bytes" -v ms="$med" 'BEGIN { if (ms > 0) printf "%.2f", (b / 1048576) / (ms / 1000); else print 0 }')"
	times_json="$(printf '%s\n' "${RUN_TIMES[@]}" | paste -sd, -)"

	emit_result "$test_name" \
		"\"status\":\"ok\",\"bytes\":$bytes,\"fixture_sha12\":\"$sha\",\"tool\":\"$TIMER_TOOL\",\"times_ms\":[$times_json],\"median_ms\":$med,\"mb_per_s\":$mbps"
	note "$test_name: median ${med}ms, ${mbps} MB/s"
}

reset_terminal() {
	# DOOM-fire dies to SIGTERM, so its own cleanup (leave alt screen, show
	# cursor) never runs. Undo it by hand.
	printf '\033[?1049l\033[?25h\033[0m'
	[ -t 0 ] && stty sane 2>/dev/null
	return 0
}

run_doom_test() {
	local test_name="doom-fire"
	if [ ! -x "$DOOM_BIN" ]; then
		emit_skip "$test_name" "DOOM-fire not built: run bench/gen-fixtures.sh"
		return
	fi
	if [ -z "$TIMEOUT_BIN" ]; then
		emit_skip "$test_name" "no timeout(1)/gtimeout(1) on PATH"
		return
	fi
	if ! command -v script >/dev/null 2>&1; then
		emit_skip "$test_name" "no script(1) on PATH"
		return
	fi
	# Upstream blocks on a "Continue?" prompt below 120x22.
	if [ "$COLS" -lt 120 ] || [ "$LINES_" -lt 22 ]; then
		emit_skip "$test_name" "terminal is ${COLS}x${LINES_}; DOOM-fire needs at least 120x22"
		return
	fi

	# DOOM-fire prints its running fps at the end of every frame, so the value
	# only exists in the byte stream. Capture it with script(1), which is a pty
	# hop every terminal pays equally, and keep just the tail: a 30s run emits
	# multiple GB and writing all of it to disk would dominate the measurement.
	local fifo="$RESULTS_DIR/.doom.$$.fifo"
	local tail_log="$RESULTS_DIR/$LABEL.doom-tail.bin"
	rm -f "$fifo"
	mkfifo "$fifo" || {
		emit_skip "$test_name" "could not create fifo $fifo"
		return
	}
	LC_ALL=C tail -c 4194304 <"$fifo" >"$tail_log" &
	local reader=$!

	# stty inside the pty pins its geometry to this terminal's, so the inner
	# pty and the visible window always agree (script(1) only copies the size
	# when its own stdin is a tty).
	local payload="stty rows $LINES_ cols $COLS 2>/dev/null; exec '$TIMEOUT_BIN' $DOOM_SECONDS '$DOOM_BIN'"
	note "doom-fire: ${DOOM_SECONDS}s at ${COLS}x${LINES_}"
	if script --version 2>/dev/null | grep -qi util-linux; then
		script -q -c "$payload" "$fifo" </dev/null
	else
		script -q "$fifo" /bin/sh -c "$payload" </dev/null
	fi
	wait "$reader" 2>/dev/null
	rm -f "$fifo"
	reset_terminal

	local fps
	fps="$(LC_ALL=C grep -ao '\[ [0-9.]\{1,\} fps \]' "$tail_log" | tail -1 |
		tr -dc '0-9.')"
	if [ -z "$fps" ]; then
		emit_skip "$test_name" "no fps marker found in the captured stream"
		return
	fi
	emit_result "$test_name" \
		"\"status\":\"ok\",\"seconds\":$DOOM_SECONDS,\"fps\":$fps,\"capture\":\"script(1) pty + tail -c 4MiB\",\"tail_log\":\"$(json_escape "$(basename "$tail_log")")\""
	note "doom-fire: $fps fps"
}

note "start: term=${TERM:-} term_program=${TERM_PROGRAM:-} ${COLS}x${LINES_} tty=$IS_TTY timer=$TIMER_TOOL"

IFS=',' read -r -a selected <<<"$TESTS"
for t in "${selected[@]}"; do
	case "$t" in
	cat-ascii) run_cat_test cat-ascii "$FIXTURE_DIR/150MB_ascii.txt" ;;
	cat-unicode) run_cat_test cat-unicode "$FIXTURE_DIR/150MB_unicode.txt" ;;
	doom-fire) run_doom_test ;;
	"") ;;
	*) note "unknown test '$t'" ;;
	esac
done

date -u +%Y-%m-%dT%H:%M:%SZ >"$DONE_MARKER"
note "done"

if [ "$IS_TTY" = true ]; then
	printf '\n\033[1;32m%s finished.\033[0m results appended to %s\n' "$LABEL" "$JSONL"
	# Give the harness a beat to notice the marker before the window closes.
	sleep 1
fi
