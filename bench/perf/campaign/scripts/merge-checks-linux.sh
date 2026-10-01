#!/usr/bin/env bash
set -u
STAGE=${STAGE:-wave3}
ulimit -n "$(ulimit -Hn)"
cd ~/dev/zz-perf-int
unset GHOSTTY_SOURCE_DIR
O=~/.cache/zz-perf/merge-$1; mkdir -p $O; shift
PRE=$1; shift; GROUPS_AB=$1; shift
ROWS="$*"
export LC_ALL=en_US.UTF-8
log() { echo "$(date +%H:%M) $*" | tee -a $O/summary.txt; }
cargo fmt --all --check > $O/fmt.log 2>&1; log "fmt exit $?"
timeout 3000 cargo clippy -j4 --workspace --all-targets --all-features -- -D warnings > $O/clippy.log 2>&1; log "clippy exit $?"
timeout 3600 cargo test -j4 --workspace --all-features --no-fail-fast > $O/test.log 2>&1; log "workspace tests exit $? ($(grep -E '^test result' $O/test.log | awk '{p+=$4;f+=$6} END {print p" passed, "f" failed"}'))"
grep -E '^test .* FAILED$' $O/test.log | sort -u > $O/test-failures.txt; log "failed tests: $(wc -l < $O/test-failures.txt)"
for t in $(awk '{print $2}' $O/test-failures.txt); do
  pkg=$(grep -B400 "^test $t .*FAILED" $O/test.log | grep -E 'Running (unittests|tests)' | tail -1 | sed -E 's/.*\(target\/debug\/deps\/([a-z_]+)-.*/\1/' | tr _ -)
  timeout 600 cargo test -j4 -p ${pkg:-zz-daemon} --all-features -- --exact "$t" > $O/solo-$t.log 2>&1; log "solo $t ($pkg) exit $?"
done
just compat-check > $O/compat-check.log 2>&1; log "compat-check exit $?"
cargo build -j4 -p zz-cli > $O/debug-build.log 2>&1; log "debug build exit $?"
ZZ_COMPAT_ZZ=$PWD/target/debug/zz_cli timeout 3600 bash compat/run.sh $ROWS > $O/compat-rows.log 2>&1; log "compat rows exit $? ($(grep -c 'yes | 0 | yes | yes | yes' $O/compat-rows.log) clean of $(echo $ROWS | wc -w))"
timeout 2400 cargo build -j4 --release -p zz-cli > $O/release.log 2>&1; log "release build exit $?"; cp target/release/zz_cli ~/.cache/zz-perf/post-$(basename $O)-cli
for i in 1 2 3; do
  ~/.cache/zz-perf/quiet-gate.sh --exec python3 bench/perf/run.py --zz $PRE --stage $STAGE --quick --only $GROUPS_AB --w0 none --json $O/ab-pre-$i.json > $O/ab-pre-$i.log 2>&1
  ~/.cache/zz-perf/quiet-gate.sh --exec python3 bench/perf/run.py --zz ~/.cache/zz-perf/post-$(basename $O)-cli --stage $STAGE --quick --only $GROUPS_AB --w0 none --json $O/ab-post-$i.json > $O/ab-post-$i.log 2>&1
  log "ab pair $i done"
done
log ALLDONE
