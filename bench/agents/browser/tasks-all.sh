#!/usr/bin/env bash
# tasks-all.sh - run every model x backend x task serially (one browser tab per backend), then summarize.
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
MODELS=${MODELS:-"gemini-3.8-flash-low gpt-oss-120b-medium"}
BACKENDS=${BACKENDS:-"own zz"}
TASKS=${TASKS:-"b1 b2 b3 b4 b5 b6 b7 b8 b9 b10"}
for m in $MODELS; do for b in $BACKENDS; do for t in $TASKS; do "$HERE/tasks.sh" "$m" "$b" "$t"; done; done; done
echo; echo "## pass by model x backend (pass/n, mean wall s, mean calls, mean failed calls)"
awk -F'\t' '{k=$1"\t"$2; n[k]++; p[k]+=$4; w[k]+=$5; c[k]+=$7; f[k]+=$8} END{for(k in n) printf "%-26s %-4s %2d/%-2d  %5.1fs  calls=%4.1f  fail=%3.1f\n", substr(k,1,index(k,"\t")-1), substr(k,index(k,"\t")+1), p[k], n[k], w[k]/n[k], c[k]/n[k], f[k]/n[k]}' "$HERE/out/tasks.tsv" | sort
echo; echo "## per task by backend"
awk -F'\t' '{k=$3"\t"$2; n[k]++; p[k]+=$4} END{for(k in n) printf "%s\t%s\t%d/%d\n", substr(k,1,index(k,"\t")-1), substr(k,index(k,"\t")+1), p[k], n[k]}' "$HERE/out/tasks.tsv" | sort -V | awk -F'\t' '{row[$1]=row[$1] sprintf("  %-4s %s", $2, $3); if(!($1 in seen)){seen[$1]=1; ord[++n]=$1}} END{for(i=1;i<=n;i++) print ord[i] row[ord[i]]}'
