#!/bin/bash
# I tasti arrivano se prima si CLICCA dentro il terminale (il fuoco del widget lo dà il clic).
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/ks}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
rm -rf "$DIR"; mkdir -p "$DIR"
ZIRKIO_STATS=1 "$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
echo "finestra=$WID"
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 400 400 click 1     # clic dentro: dà il fuoco al widget
sleep 1
shot() { import -window root "$DIR/$1.png"; }

shot 01-prima
xdotool type --delay 30 'echo ciao-dal-terminale'
xdotool key Return
sleep 2
shot 02-scritto
echo "diff dopo la scrittura: $(magick compare -metric AE "$DIR/01-prima.png" "$DIR/02-scritto.png" null: 2>&1)"

xdotool key ctrl+shift+d
sleep 2
shot 03-divisione
echo "diff dopo ctrl+maiusc+d: $(magick compare -metric AE "$DIR/02-scritto.png" "$DIR/03-divisione.png" null: 2>&1)"

xdotool key ctrl+shift+t
sleep 2
shot 04-scheda
echo "diff dopo ctrl+maiusc+t: $(magick compare -metric AE "$DIR/03-divisione.png" "$DIR/04-scheda.png" null: 2>&1)"

kill $APP 2>/dev/null
echo "--- log:"; grep -E "selftest|divisione" "$DIR/app.log" | head -5
exit 0
