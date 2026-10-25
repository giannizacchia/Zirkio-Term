#!/bin/bash
# Tasto destro dentro il terminale: si apre il menu e le voci funzionano?
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
ZIRKIO_STATS=1 "$BIN" >/tmp/tasto3.log 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 300 300 click 1; sleep 1      # fuoco nel terminale
import -window root /tmp/t3-prima.png
xdotool mousemove 300 300 click 3; sleep 1.5    # tasto destro -> menu
import -window root /tmp/t3-menu.png
echo "menu aperto? differenza pixel: $(magick compare -metric AE /tmp/t3-prima.png /tmp/t3-menu.png null: 2>&1)"
# la prima voce ("Nuova scheda") sta poco sotto il puntatore
xdotool mousemove 360 316 click 1; sleep 2
echo "dopo il clic sulla prima voce:"; grep -E "azione|fuoco" /tmp/tasto3.log | tail -3
import -window root /tmp/t3-dopo.png
kill $APP 2>/dev/null
exit 0
