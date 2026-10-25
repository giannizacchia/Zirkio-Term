#!/bin/bash
# Perche' la finestra non si vede con un window manager? (stato GTK + albero X)
set -u
export NO_AT_BRIDGE=1
export GSETTINGS_BACKEND=memory
BIN="${BIN:-./target/release/zirkio-term}"
openbox >/tmp/wm.log 2>&1 &
WM=$!
sleep 1
ZIRKIO_STATS=1 "$BIN" >/tmp/g.log 2>&1 &
APP=$!
sleep 6
echo "== log app:"; cat /tmp/g.log
echo "== albero X:"
xwininfo -root -tree 2>&1 | grep -E "0x[0-9a-f]+ |children" | head -25
echo "== root id: $(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')"
import -window root /tmp/root.png 2>/dev/null
echo "== pixel schermo: $(magick /tmp/root.png -format 'mean=%[fx:mean]' info: 2>&1)"
kill $APP $WM 2>/dev/null
exit 0
