#!/bin/bash
# Guarda la finestra dell'app: geometria, stato, pixel.
set -u
export NO_AT_BRIDGE=1
export GSETTINGS_BACKEND=memory
BIN="${BIN:-./target/release/zirkio-term}"
openbox >/tmp/wm.log 2>&1 &
WM=$!
sleep 1
"$BIN" >/tmp/g.log 2>&1 &
APP=$!
sleep 5
WID=$(xdotool search --onlyvisible --name "" 2>/dev/null | tail -1)
echo "== id finestra: $WID"
xwininfo -id "$WID" 2>&1 | sed -n '1,20p'
import -window "$WID" /tmp/win.png 2>&1
echo "== pixel finestra: $(magick /tmp/win.png -format '%wx%h mean=%[fx:mean]' info: 2>&1)"
import -window root /tmp/root.png 2>&1
echo "== pixel schermo: $(magick /tmp/root.png -format '%wx%h mean=%[fx:mean]' info: 2>&1)"
echo "== xdotool getwindowname: $(xdotool getwindowname "$WID" 2>&1)"
echo "== log app:"; head -20 /tmp/g.log
kill $APP $WM 2>/dev/null
exit 0
