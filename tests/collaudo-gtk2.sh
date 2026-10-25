#!/bin/bash
# Collaudo GTK3 in Xvfb SENZA window manager (con un WM la finestra non viene mappata da openbox):
# la finestra si trova dall'albero X (la piu' larga), poi si manda il fuoco e i tasti.
# Uso: xvfb-run -a -s "-screen 0 1200x760x24" bash tests/collaudo-gtk2.sh
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/zt}"
export NO_AT_BRIDGE=1
export GSETTINGS_BACKEND=memory

rm -rf "$DIR"; mkdir -p "$DIR"
"$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 6
if ! kill -0 $APP 2>/dev/null; then
  echo "L'APP E' MORTA"; cat "$DIR/app.log"; exit 1
fi

# La finestra piu' larga dell'albero (esclude i 1x1 di servizio).
WID=$(xwininfo -root -tree 2>/dev/null \
      | awk '/0x[0-9a-f]+/ { for (i = 1; i <= NF; i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i, d, "x"); if (d[1] + 0 > best) { best = d[1] + 0; id = $1 } } } END { print id }')
echo "finestra: ${WID:-nessuna}"
if [ -n "${WID:-}" ]; then
  xdotool windowfocus "$WID" 2>/dev/null || true
  xdotool windowraise "$WID" 2>/dev/null || true
fi

shot() { import -window root "$DIR/$1.png"; echo "  -> $1.png (media $(magick "$DIR/$1.png" -format '%[fx:mean]' info: 2>/dev/null))"; }

sleep 1
shot 01-avvio

xdotool type --delay 40 'echo "prova: funziona"; ls --color=always /etc | head -6'
xdotool key Return
sleep 2
shot 02-scrittura

xdotool key ctrl+shift+t
sleep 2
shot 03-seconda-scheda

xdotool key ctrl+shift+d
sleep 2
shot 04-divisione

xdotool key F2
sleep 1.5
shot 05-tema-chiaro

xdotool key F2
sleep 1
xdotool key F1
sleep 1.5
shot 06-guida
xdotool key Escape
sleep 1

kill $APP 2>/dev/null
echo "--- log app ---"
head -40 "$DIR/app.log"
exit 0
