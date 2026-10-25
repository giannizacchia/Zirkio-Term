#!/bin/bash
# 1) tre schede: larghezza delle etichette  2) `exit` in un pannello lo chiude
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11

echo "=== tre schede"
ZIRKIO_STATS=1 ZIRKIO_SELFTEST=tab,tab timeout 12 "$BIN" 2>&1 | grep "misura vista" | tail -3

echo "=== split + exit nel pannello di destra"
ZIRKIO_STATS=1 ZIRKIO_SELFTEST=split-right "$BIN" >/tmp/esci.log 2>&1 &
APP=$!
sleep 7
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 800 300 click 1; sleep 1     # pannello di destra
xdotool type --delay 40 "exit"; xdotool key Return; sleep 3
echo "pannelli dopo exit: $(grep -c 'processo finito' /tmp/esci.log) chiusure; ultimo stato:"
grep -E "processo finito|azione ClosePane" /tmp/esci.log | tail -3
import -window root /tmp/esci.png
kill $APP 2>/dev/null
exit 0
