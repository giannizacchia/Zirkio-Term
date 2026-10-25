#!/bin/bash
# Diagnostica: chi c'è sullo schermo X?
set -u
export NO_AT_BRIDGE=1
export GSETTINGS_BACKEND=memory
BIN="${BIN:-./target/release/zirkio-term}"

echo "== DISPLAY=$DISPLAY"
openbox >/tmp/wm.log 2>&1 &
WM=$!
sleep 1
echo "== wmctrl -m:"; wmctrl -m 2>&1 | head -5
"$BIN" >/tmp/g.log 2>&1 &
APP=$!
sleep 5
echo "== vivo: $(kill -0 $APP 2>/dev/null && echo si || echo no)"
echo "== wmctrl -l:"; wmctrl -l 2>&1
echo "== albero X (prime righe):"
xwininfo -root -tree 2>&1 | head -25
echo "== xdotool search --onlyvisible --name '':"
xdotool search --onlyvisible --name "" 2>&1 | head -5
echo "== log app:"; head -30 /tmp/g.log
echo "== log wm:"; head -10 /tmp/wm.log
kill $APP $WM 2>/dev/null
exit 0
