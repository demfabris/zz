#!/bin/sh
link() {
    printf '\033]8;;%s\007%s\033]8;;\007' "$1" "$2"
}
printf '\033]133;A\007%% prompt\r\n'
printf '\033]133;C\007output\r\n'
printf '%090d\r\n' 0
link http://a aa
printf ' '
link http://b bb
printf '\r\n'
link http://a again
link http://c cc
printf '\r\n'
link http://w "$(printf '%0100d' 0)"
printf '\r\n'
link 'http://x\y' ab
link http://y cd
printf '\r\nNEXT'
exec sleep 600
