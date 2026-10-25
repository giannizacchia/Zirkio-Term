#!/bin/bash
# La scheda nuova si VEDE? (differenza solo nella striscia delle schede)
set -u
BIN="${BIN:-./target/release/zirkio-term}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
ZIRKIO_STATS=1 ZIRKIO_SELFTEST=tab "$BIN" >/tmp/scheda.log 2>&1 &
APP=$!
sleep 1.6
import -window root /tmp/s1.png
sleep 3            # a 2,5 s scatta l'autotest
import -window root /tmp/s2.png
kill $APP 2>/dev/null
sleep 1
# la striscia delle schede sta sotto l'HeaderBar: misuro solo quella fascia
for f in s1 s2; do magick /tmp/$f.png -crop 1070x40+0+40 +repage /tmp/$f-tab.png; done
echo "differenza nella striscia delle schede: $(magick compare -metric AE /tmp/s1-tab.png /tmp/s2-tab.png null: 2>&1)"
echo "--- log:"; grep -E "selftest|vista attiva" /tmp/scheda.log | head -6
exit 0
