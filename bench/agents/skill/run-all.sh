#!/usr/bin/env bash
S=$(cd "$(dirname "$0")" && pwd)
MODELS=${MODELS:-"gemini-3.6-flash-low gpt-oss-120b-medium gemini-3.8-flash-low"}
ARMS=${ARMS:-"none skill inline"}
TASKS=${TASKS:-"t1 t2 t3 t4 t5 t6 t7 t9 t8"}
PAR=${PAR:-3}
for m in $MODELS; do for a in $ARMS; do for t in $TASKS; do echo "$m $a $t"; done; done; done | xargs -P "$PAR" -L1 "$S/run-one.sh"
"$S/summary.sh"
