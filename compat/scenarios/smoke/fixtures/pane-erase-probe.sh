#!/bin/sh
stty -a | tr '\n' ' ' | sed -n 's/.* erase = \([^;]*\);.*/ERASE[\1]/p'
stty -echo -icanon min 1 time 0
exec cat -v
