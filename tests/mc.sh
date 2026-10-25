#!/bin/bash
# mc dentro zirkio-term: linee dei riquadri, colori, barra dei tasti funzione.
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/mc}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
rm -rf "$DIR"; mkdir -p "$DIR"
"$BIN" -e mc >"$DIR/app.log" 2>&1 &
APP=$!
sleep 7
import -window root "$DIR/mc.png"
echo "media=$(magick "$DIR/mc.png" -format '%[fx:mean]' info:)"
kill $APP 2>/dev/null
echo "--- log:"; head -10 "$DIR/app.log"
exit 0
