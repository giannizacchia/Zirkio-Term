#!/bin/bash
# Riproduce i due difetti segnalati senza tastiera:
#  A) linee e simboli (charset DEC + UTF-8)
#  B) divisione a destra/in basso (autotest ZIRKIO_SELFTEST)
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/bug}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11

rm -rf "$DIR"; mkdir -p "$DIR"

# --- A) disegno delle linee
"$BIN" -e /bin/sh "$PWD/tests/box-test.sh" >"$DIR/a.log" 2>&1 &
A=$!
sleep 6
import -window root "$DIR/A-linee.png"
echo "A-linee: media=$(magick "$DIR/A-linee.png" -format '%[fx:mean]' info:)"
kill $A 2>/dev/null
sleep 1

# --- B) divisione (poi tema, per vedere che anche quello va)
ZIRKIO_STATS=1 ZIRKIO_SELFTEST=split-right,split-down,theme "$BIN" >"$DIR/b.log" 2>&1 &
B=$!
sleep 9
import -window root "$DIR/B-divisione.png"
echo "B-divisione: media=$(magick "$DIR/B-divisione.png" -format '%[fx:mean]' info:)"
kill $B 2>/dev/null
sleep 1
echo "--- log B:"; grep -E "selftest|divisione" "$DIR/b.log" | head -20
echo "--- log A:"; head -5 "$DIR/a.log"
exit 0
