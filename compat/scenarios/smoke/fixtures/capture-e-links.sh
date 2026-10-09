#!/bin/sh
link() {
    printf '\033]8;;%s\007%s\033]8;;\007' "$1" "$2"
}
link http://a aa
printf ' '
link http://b bb
printf '\r\n'
link http://a again
link http://c cc
printf '\r\n'
printf '\033[1;31m'
link http://s red
printf '\033[0m tail\r\n'
printf '\033]8;;http://m\007x\033[4my\033[24mz\033]8;;\007\033[32mgreen\033[0m\r\n'
printf '\033]8;;http://q\007a\033[1mb\033]8;;\007\033[0m\r\n'
link 'http://x\y' ab
printf ' '
link http://y cd
printf '\r\n'
printf 'pre '
link http://w "$(printf '%090d' 0)"
printf ' post\r\n'
link http://t 'ab  '
printf '\r\n'
printf 'x'
link http://z y
printf '\r\n'
link http://u "$(printf '\344\270\200\344\272\214')"
printf 'z\r\n'
printf '\033]8;;http://n\007one\r\ntwo\033]8;;\007 three\r\n'
printf '\033[7m'
link http://r rev
printf '\r\nNEXT'
exec sleep 600
