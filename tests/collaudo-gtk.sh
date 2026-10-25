#!/bin/bash
# Collaudo della versione GTK3 dentro Xvfb + openbox.
# La finestra si sceglie dalla GEOMETRIA (senza WM il nome non basta).
# Uso: xvfb-run -a -s "-screen 0 1200x760x24" bash tests/collaudo-gtk.sh
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/zt}"
export NO_AT_BRIDGE=1
export GSETTINGS_BACKEND=memory

rm -rf "$DIR"; mkdir -p "$DIR"
openbox >"$DIR/wm.log" 2>&1 &
WM=$!
sleep 1
"$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 6
if ! kill -0 $APP 2>/dev/null; then
  echo "L'APP E' MORTA"; cat "$DIR/app.log"; kill $WM 2>/dev/null; exit 1
fi

# La finestra piu' larga fra quelle visibili e' la nostra.
WID=""
BIGGEST=0
for w in $(xdotool search --onlyvisible --name "" 2>/dev/null); do
  ww=$(xdotool getwindowgeometry --shell "$w" 2>/dev/null | awk -F= '/^WIDTH/{print $2}')
  [ -n "${ww:-}" ] || continue
  if [ "$ww" -gt "$BIGGEST" ]; then BIGGEST=$ww; WID=$w; fi
done
echo "finestra: ${WID:-nessuna} (larghezza $BIGGEST)"
if [ -n "${WID:-}" ]; then
  xdotool windowactivate --sync "$WID" 2>/dev/null || xdotool windowfocus "$WID" 2>/dev/null || true
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
sleep 1
kill $WM 2>/dev/null
echo "--- log app ---"
head -40 "$DIR/app.log"
exit 0
