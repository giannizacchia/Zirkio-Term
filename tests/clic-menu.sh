#!/bin/bash
# Il menu "dividi" dell'HeaderBar funziona? Clicco i pulsanti e guardo il log.
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/clk}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
rm -rf "$DIR"; mkdir -p "$DIR"
ZIRKIO_STATS=1 "$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
xdotool windowfocus "$WID" 2>/dev/null
geo=$(xdotool getwindowgeometry --shell "$WID")
WX=$(echo "$geo" | awk -F= '/^X=/{print $2}')
WY=$(echo "$geo" | awk -F= '/^Y=/{print $2}')
echo "finestra a $WX,$WY"

# I pulsanti dell'HeaderBar stanno in alto a sinistra: provo alcune ascisse.
for dx in 30 50 70 90 110; do
  xdotool mousemove $((WX + dx)) $((WY + 22)) click 1
  sleep 1
  import -window root "$DIR/c-$dx.png"
  # se si è aperto un menu, clicco la prima voce poco sotto
  xdotool mousemove $((WX + dx)) $((WY + 60)) click 1
  sleep 1
  if grep -q "azione Split" "$DIR/app.log"; then
    echo "dx=$dx -> menu dividi FUNZIONA: $(grep 'azione Split' "$DIR/app.log" | tail -1)"
    break
  else
    echo "dx=$dx -> nessuna azione"
  fi
  xdotool key Escape
  sleep 0.3
done

echo "--- azioni registrate:"
grep "azione" "$DIR/app.log" | head -5
kill $APP 2>/dev/null
exit 0
