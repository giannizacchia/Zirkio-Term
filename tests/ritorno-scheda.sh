#!/bin/bash
# Scenario dell'utente: seconda scheda, ritorno sulla prima, il prompt risponde ancora?
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
ZIRKIO_STATS=1 "$BIN" >/tmp/ritorno.log 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 300 400 click 1; sleep 1

xdotool type --delay 40 "echo PRIMO"; xdotool key Return; sleep 1.5
xdotool key ctrl+shift+t; sleep 2.5                       # seconda scheda
xdotool type --delay 40 "echo SECONDO"; xdotool key Return; sleep 1.5
xdotool key ctrl+Next; sleep 2                            # torno alla prima
import -window root /tmp/r1.png
xdotool type --delay 40 "echo TORNATO"; xdotool key Return; sleep 2
import -window root /tmp/r2.png
echo "la prima scheda risponde? differenza pixel dopo un comando: $(magick compare -metric AE /tmp/r1.png /tmp/r2.png null: 2>&1)"
# ritorno alla seconda COL MOUSE (clic sull'etichetta della scheda) e verifico che risponda
xdotool mousemove 200 72 click 1; sleep 2
import -window root /tmp/r3.png
xdotool type --delay 40 "echo ANCORA"; xdotool key Return; sleep 2
import -window root /tmp/r4.png
echo "la seconda scheda risponde al clic sul nome? differenza: $(magick compare -metric AE /tmp/r3.png /tmp/r4.png null: 2>&1)"
echo "--- log:"; grep -E "azione|fuoco" /tmp/ritorno.log | tail -5
kill $APP 2>/dev/null
exit 0
