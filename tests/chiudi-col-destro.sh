#!/bin/bash
# Tasto destro su un pannello: la PRIMA voce ("Chiudi questo pannello") lo chiude.
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
ZIRKIO_STATS=1 ZIRKIO_SELFTEST=split-right "$BIN" >/tmp/chiudi.log 2>&1 &
APP=$!
sleep 7
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 300 300 click 1; sleep 1          # fuoco nel pannello sinistro
xdotool mousemove 300 400 click 3; sleep 1.5        # tasto destro -> popover
import -window root /tmp/ch-menu.png
xdotool mousemove 300 420 click 1; sleep 2          # prima voce
echo "--- chiusure:"; grep -E "chiudo il pannello|processo finito" /tmp/chiudi.log | tail -3
import -window root /tmp/ch-dopo.png
kill $APP 2>/dev/null
exit 0
