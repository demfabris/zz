#!/opt/homebrew/bin/bash
set -u
STAGE=${STAGE:-final}
cd ${WT:-~/dev/zz-perf-int}
unset GHOSTTY_SOURCE_DIR
export PATH=/opt/homebrew/opt/coreutils/libexec/gnubin:/opt/homebrew/opt/gnu-sed/libexec/gnubin:/opt/homebrew/opt/grep/libexec/gnubin:/opt/homebrew/bin:$PATH LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8
O=/tmp/zzpc/merge-$1; mkdir -p $O; shift
PRE=$1; shift; GROUPS_AB=$1; shift
ROWS="$*"
log() { echo "$(date +%H:%M) $*" | tee -a $O/summary.txt; }
cargo fmt --all --check > $O/fmt.log 2>&1; log "fmt exit $?"
timeout 3000 cargo clippy --workspace --all-targets --all-features -- -D warnings > $O/clippy.log 2>&1; log "clippy exit $?"
timeout 3600 cargo test --workspace --all-features --no-fail-fast > $O/test.log 2>&1; log "workspace tests exit $? ($(grep -E '^test result' $O/test.log | awk '{p+=$4;f+=$6} END {print p" passed, "f" failed"}'))"
grep -E '^test .* FAILED$|^    [a-z_:]+$' $O/test.log | sort -u > $O/test-failures.txt; log "failed tests: $(grep -c FAILED $O/test-failures.txt)"
for t in $(grep FAILED $O/test-failures.txt | awk '{print $2}'); do
  run=$(grep -B200 "^test $t .*FAILED" $O/test.log | grep -E 'Running (unittests|tests)' | tail -1)
  pkg=$(echo "$run" | sed -E 's/.*\(target\/debug\/deps\/([a-z_]+)-.*/\1/' | tr _ -)
  target=$(echo "$run" | sed -nE 's/.*Running tests\/([a-z0-9_]+)\.rs .*/\1/p')
  sel="-p ${pkg:-zz-daemon}"; [ -n "$target" ] && sel="--workspace --test $target"
  timeout 600 cargo test $sel --all-features -- --exact "$t" > $O/solo-$t.log 2>&1; log "solo $t ($sel) exit $?"
done
just compat-check > $O/compat-check.log 2>&1; log "compat-check exit $?"
cargo build -p zz-cli > $O/debug-build.log 2>&1; log "debug build exit $?"
PATH=/opt/homebrew/bin:$PATH ZZ_COMPAT_ZZ=$PWD/target/debug/zz_cli timeout 3600 /opt/homebrew/bin/bash compat/run.sh $ROWS > $O/compat-rows.log 2>&1; log "compat rows exit $? ($(grep -c 'yes | 0 | yes | yes | yes' $O/compat-rows.log) clean of $(echo $ROWS | wc -w))"
just web-build > $O/web-build.log 2>&1; log "web-build exit $?"
timeout 2400 cargo build --release -p zz-cli > $O/release.log 2>&1; log "release build exit $?"; cp target/release/zz_cli /tmp/zzpc/post-$(basename $O)-cli
if [ "${WIRE:-0}" = 1 ]; then
  timeout 1800 /opt/homebrew/bin/bash compat/attached-client.sh $PWD/target/debug/zz_cli $PWD/compat/.cache/tmux-src/tmux > $O/attached-client.log 2>&1; log "attached-client exit $?"
  timeout 1800 /opt/homebrew/bin/bash compat/tui-screen-diff.sh $PWD/target/debug/zz_cli $PWD/compat/.cache/tmux-src/tmux > $O/tui-screen.log 2>&1; log "tui-screen-diff exit $?"
  timeout 2400 just ios build iPad > $O/ios.log 2>&1; log "ios-gpui iPad build exit $?"
fi
for i in 1 2 3; do
  python3 bench/perf/run.py --zz $PRE --stage $STAGE --quick --only $GROUPS_AB --w0 none --json $O/ab-pre-$i.json > $O/ab-pre-$i.log 2>&1
  python3 bench/perf/run.py --zz /tmp/zzpc/post-$(basename $O)-cli --stage $STAGE --quick --only $GROUPS_AB --w0 none --json $O/ab-post-$i.json > $O/ab-post-$i.log 2>&1
  log "ab pair $i done"
done
log ALLDONE
