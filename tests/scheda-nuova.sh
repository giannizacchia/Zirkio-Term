#!/bin/bash
# La scheda nuova si crea davvero? (autotest: tab, poi split)
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/ks5}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
rm -rf "$DIR"; mkdir -p "$DIR"
ZIRKIO_STATS=1 ZIRKIO_SELFTEST=tab "$BIN" >"$DIR/app.log" 2>&1 &
APP=$!
sleep 7
import -window root "$DIR/dopo-tab.png"
echo "media=$(magick "$DIR/dopo-tab.png" -format '%[fx:mean]' info:)"
kill $APP 2>/dev/null
sleep 1
grep -E "selftest|finestra:" "$DIR/app.log" | head -10
exit 0
