#!/bin/bash
# Collaudo interattivo dentro Xvfb: scrive, apre schede, divide, apre il menu, cambia tema.
# Uso: xvfb-run -a -s "-screen 0 1200x760x24" bash tests/collaudo-interattivo.sh
set -u
BIN="${BIN:-./target/debug/zirkio-term}"
DIR="${DIR:-/tmp/zt}"

rm -rf "$DIR"; mkdir -p "$DIR"
"$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 5
if ! kill -0 $APP 2>/dev/null; then
  echo "L'APP È MORTA"; cat "$DIR/app.log"; exit 1
fi
WID=$(xdotool search --pid $APP 2>/dev/null | tail -1)
if [ -z "${WID:-}" ]; then WID=$(xdotool search --name 'Terminale|zirkio|@' 2>/dev/null | tail -1); fi
echo "finestra: ${WID:-nessuna}"
if [ -z "${WID:-}" ]; then xwininfo -root -tree | head -20; fi
if [ -n "${WID:-}" ]; then
  xdotool windowfocus "$WID" 2>/dev/null || true
  xdotool windowraise "$WID" 2>/dev/null || true
fi
shot() { import -window root "$DIR/$1.png"; echo "  -> $1.png"; }
sleep 1
shot 01-avvio

xdotool type --delay 40 'echo "prova di scrittura: funziona"; ls --color=always /etc | head -6'
xdotool key Return
sleep 1.5
shot 02-scrittura

xdotool key ctrl+shift+t
sleep 2
shot 03-seconda-scheda

xdotool key ctrl+shift+d
sleep 2
shot 04-divisione

xdotool key F10
sleep 1
shot 05-menu

xdotool key Escape
sleep 0.5
xdotool key F2
sleep 1.5
shot 06-tema-chiaro

xdotool key F2
sleep 1
xdotool key F1
sleep 1
shot 07-guida

kill $APP 2>/dev/null
sleep 1
echo "--- log app ---"
head -40 "$DIR/app.log"
exit 0
