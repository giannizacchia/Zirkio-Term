#!/bin/bash
# I pulsanti dell'HeaderBar ricevono il clic? Scansione dell'ascissa e log delle azioni.
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
ZIRKIO_STATS=1 "$BIN" >/tmp/head.log 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
read -r WX WY WW WH <<<"$(xdotool getwindowgeometry --shell "$WID" | awk -F= '/^X=/{x=$2}/^Y=/{y=$2}/^WIDTH=/{w=$2}/^HEIGHT=/{h=$2}END{print x, y, w, h}')"
echo "finestra $WX,$WY ${WW}x${WH}"
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove $((WX + WW / 2)) $((WY + WH / 2)) click 1   # fuoco nel terminale
sleep 1
for y in 12 20 26; do
  for dx in $(seq 8 8 200); do
    xdotool mousemove $((WX + dx)) $((WY + y)) click 1
    sleep 0.25
    if grep -q "azione" /tmp/head.log; then
      echo "CLIC UTILE: x=$dx y=$y -> $(grep 'azione' /tmp/head.log | tail -1 | cut -c1-60)"
      xdotool key Escape; sleep 0.2
    fi
  done
done
echo "--- azioni registrate:"; grep "azione" /tmp/head.log | head -8
kill $APP 2>/dev/null
exit 0
