#!/bin/bash
# Costo CPU per far passare 400.000 righe nel terminale (misura end-to-end).
# uso: misura-cpu.sh <binario>
set -u
BIN="${1:?serve il binario}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
HZ=$(getconf CLK_TCK)
"$BIN" -e /bin/sh -c 'seq 1 400000; sleep 60' >/dev/null 2>&1 &
APP=$!
sleep 1
T0=$(awk '{print $14+$15}' /proc/$APP/stat 2>/dev/null || echo 0)
sleep 11
T1=$(awk '{print $14+$15}' /proc/$APP/stat 2>/dev/null || echo 0)
RSS=$(awk '/VmRSS/{print $2}' /proc/$APP/status 2>/dev/null || echo 0)
awk -v a="$T0" -v b="$T1" -v hz="$HZ" -v rss="$RSS" -v bin="$BIN" 'BEGIN{
  cpu=(b-a)/hz;
  printf "%-28s CPU %.2f s per 400.000 righe (%.0f righe/s) · RSS %d kB\n", bin, cpu, 400000/cpu, rss
}'
kill $APP 2>/dev/null
exit 0
