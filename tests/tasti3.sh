#!/bin/bash
# Quali scorciatoie arrivano? (dopo un clic dentro il terminale)
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/ks2}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
rm -rf "$DIR"; mkdir -p "$DIR"
"$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 6
ROOT=$(xwininfo -root 2>/dev/null | awk '/Window id/{print $4}')
WID=$(xwininfo -root -tree 2>/dev/null | awk -v root="$ROOT" '/0x[0-9a-f]+/ && $1 != root { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
xdotool windowfocus "$WID" 2>/dev/null
xdotool mousemove 400 400 click 1
sleep 1

prova() {   # nome, poi i tasti
  local nome="$1"; shift
  import -window root "$DIR/p-$nome-a.png"
  xdotool key "$@"
  sleep 2
  import -window root "$DIR/p-$nome-b.png"
  local d
  d=$(magick compare -metric AE "$DIR/p-$nome-a.png" "$DIR/p-$nome-b.png" null: 2>&1)
  echo "$nome: cambiato=$d"
}

prova nuova-scheda ctrl+shift+t
prova split-giu ctrl+shift+e
prova tema F2
prova guida F1
xdotool key Escape; sleep 1
prova scheda-succ ctrl+Page_Down
prova carattere-piu ctrl+shift+plus

kill $APP 2>/dev/null
exit 0
