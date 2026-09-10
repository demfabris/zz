#!/usr/bin/env bash
S=$(cd "$(dirname "$0")" && pwd)
echo "## pass rate by model x arm (pass/n, mean wall s, mean zz calls, mean failed zz calls)"
awk -F'\t' '{k=$1"\t"$2; n[k]++; p[k]+=$4; w[k]+=$5; c[k]+=$7; f[k]+=$8} END{for(k in n) printf "%-26s %-7s %2d/%-2d  %5.1fs  zz=%4.1f  fail=%3.1f\n", substr(k,1,index(k,"\t")-1), substr(k,index(k,"\t")+1), p[k], n[k], w[k]/n[k], c[k]/n[k], f[k]/n[k]}' "$S/out/results.tsv" | sort
echo; echo "## per task (pass count over runs) by arm"
awk -F'\t' '{k=$3"\t"$2; n[k]++; p[k]+=$4} END{for(k in n) printf "%s\t%s\t%d/%d\n", substr(k,1,index(k,"\t")-1), substr(k,index(k,"\t")+1), p[k], n[k]}' "$S/out/results.tsv" | sort | awk -F'\t' '{row[$1]=row[$1] sprintf("  %-6s %s", $2, $3)} END{for(t in row) print t row[t]}' | sort
