#!/usr/bin/env bash
set -u
W=${W:-/home/demfabris/dev/zz-echomap}
D=${D:-/tmp/zzpc/echomap}
BASE=${BASE:-/home/demfabris/dev/zz-perf-int/target/release/zz_cli}
T=$W/compat/.cache/tmux-src/tmux
DRV=$W/bench/perf/campaign/w4-echomap.py
C=$W/bench/perf/campaign
mkdir -p $D
gcc -O2 -o $D/hopprobe $C/w4-echomap-hop.c -lpthread || exit 1
gcc -O2 -shared -fPIC -o $D/sct.so $C/w4-echomap-sct.c -ldl -lpthread || exit 1
cd $W
ulimit -n $(ulimit -Hn)
run() { local out=$1; shift; echo "== $out $(date +%T) load $(cut -d' ' -f1 /proc/loadavg)"; env "$@" timeout 240 python3 $DRV ${ZZB:-$D/dl4-cli} $T $D/$out.json 2>&1 | tail -4; }
probe() { local name=$1 bin=$2; rm -rf $D/pd-$name; mkdir -p $D/pd-$name; ZZB=$bin run q-probe-$name NOIDLE=1 ONLY=zz "MUX_ENV={\"zz\":{\"ZZ_ECHOPROBE\":\"$D/pd-$name\",\"LD_PRELOAD\":\"$D/sct.so\",\"SCT_DIR\":\"$D/pd-$name\"}}"; }
timeout 200 $D/hopprobe 30000 | tee $D/hop-idle.txt
timeout 60 $D/hopprobe 0 | tee $D/hop-warm.txt
run q-clean-dl4 NOIDLE=1 ONLY=zz
ZZB=$BASE run q-clean-base NOIDLE=1 ONLY=zz
run q-clean-tmux NOIDLE=1 ONLY=tmux
run q-nogather-dl4 NOIDLE=1 ONLY=zz 'MUX_ENV={"zz":{"ZZ_PTY_GATHER":"0"}}'
run q-pin1 NOIDLE=1 VARIANTS=idle:0 'AFFINITY=bench=2;client=2;server=2;pane=2'
run q-pinsrv-dl4 NOIDLE=1 ONLY=zz VARIANTS=idle:0 'AFFINITY=server=2'
run q-spin NOIDLE=1 VARIANTS=idle:0 SPIN=16
ZZB=$BASE run q-spin-base NOIDLE=1 ONLY=zz VARIANTS=idle:0 SPIN=16
[ -x $D/dl4-probe-cli ] && probe dl4 $D/dl4-probe-cli
[ -x $D/base-probe-cli ] && probe base $D/base-probe-cli
rm -rf $D/pd-tmux; mkdir -p $D/pd-tmux
run q-probe-tmux NOIDLE=1 ONLY=tmux "MUX_ENV={\"tmux\":{\"LD_PRELOAD\":\"$D/sct.so\",\"SCT_DIR\":\"$D/pd-tmux\"}}"
echo "series done $(date +%T)"
