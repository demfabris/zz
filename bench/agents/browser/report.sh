#!/usr/bin/env bash
# report.sh [results.tsv] - render the parity matrix as markdown: one row per primitive, one column per backend.
set -uo pipefail
RESULTS=${1:-$(cd "$(dirname "$0")" && pwd)/out/results.tsv}
[ -s "$RESULTS" ] || { echo "no results at $RESULTS" >&2; exit 1; }
awk -F'\t' '
{ backend[$1]=1; if (!($2 in order)) { order[$2]=++n; names[n]=$2 }; pass[$2,$1]=$3; ms[$2,$1]=$4; note[$2,$1]=$5 }
END {
  nb=0; for (b in backend) cols[++nb]=b; for (i=1;i<=nb;i++) for (j=i+1;j<=nb;j++) if (cols[j] < cols[i]) { t=cols[i]; cols[i]=cols[j]; cols[j]=t }
  printf "| primitive |"; for (i=1;i<=nb;i++) printf " %s |", cols[i]; printf " note |\n"
  printf "|---|"; for (i=1;i<=nb;i++) printf "---|"; printf "---|\n"
  for (k=1;k<=n;k++) { p=names[k]; printf "| %s |", p; worst="";
    for (i=1;i<=nb;i++) { b=cols[i]; if ((p,b) in pass) { printf " %s %s ms |", (pass[p,b]==1?"pass":"FAIL"), ms[p,b]; if (pass[p,b]!=1 && note[p,b]!="") worst=worst b ": " substr(note[p,b],1,90) " " } else printf " - |" }
    printf " %s |\n", worst }
  printf "\n"; for (i=1;i<=nb;i++) { b=cols[i]; t=0; ok=0; for (k=1;k<=n;k++) if ((names[k],b) in pass) { t++; ok+=(pass[names[k],b]==1) }; printf "%s: %d/%d pass\n", b, ok, t }
}' "$RESULTS"
