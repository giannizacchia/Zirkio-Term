#!/bin/bash
# I tasti arrivano al terminale? Prova con fuoco normale e con --window.
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
"$BIN" >/tmp/k.log 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
echo "finestra=$WID"
xdotool windowfocus "$WID" 2>/dev/null
echo "fuoco ora: $(xdotool getwindowfocus 2>&1)"
sleep 1
import -window root /tmp/k0.png
echo "prima:  media=$(magick /tmp/k0.png -format '%[fx:mean]' info:)"

# 1) XTEST sul fuoco
xdotool type --delay 40 'echo prova-xtest'
xdotool key Return
sleep 2
import -window root /tmp/k1.png
echo "xtest:  media=$(magick /tmp/k1.png -format '%[fx:mean]' info:)"

# 2) eventi inviati direttamente alla finestra
xdotool key --window "$WID" e
xdotool key --window "$WID" c
xdotool key --window "$WID" h
xdotool key --window "$WID" o
xdotool key --window "$WID" space
xdotool key --window "$WID" p
xdotool key --window "$WID" r
xdotool key --window "$WID" o
xdotool key --window "$WID" v
xdotool key --window "$WID" a
xdotool key --window "$WID" Return
sleep 2
import -window root /tmp/k2.png
echo "window: media=$(magick /tmp/k2.png -format '%[fx:mean]' info:)"
echo "diff k1-k2: $(magick compare -metric AE /tmp/k1.png /tmp/k2.png null: 2>&1)"
kill $APP 2>/dev/null
echo "--- log:"; head -10 /tmp/k.log
exit 0
