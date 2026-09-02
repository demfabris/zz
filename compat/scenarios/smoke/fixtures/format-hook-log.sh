#!/bin/sh
# Append one already expanded hook line to a log. The hook body passes the log
# path first so the two sides can keep separate files under one HOME.
log="$1"
shift
printf '%s\n' "$*" >>"$log"
