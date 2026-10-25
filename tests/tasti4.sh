#!/bin/bash
# Che keysym arriva per ogni scorciatoia? (log del programma)
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/ks4}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
rm -rf "$DIR"; mkdir -p "$DIR"
ZIRKIO_STATS=1 "$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
echo "finestra=$WID"
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 400 400 click 1
sleep 1
for k in ctrl+shift+t ctrl+shift+d ctrl+shift+e F2; do
  xdotool key "$k"
  sleep 1
done
sleep 1
kill $APP 2>/dev/null
echo "--- tasti visti dal programma:"
grep "tasto keyval" "$DIR/app.log"
exit 0
