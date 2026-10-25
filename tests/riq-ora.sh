#!/bin/bash
# Riquadri di prova nel build attuale: fotogramma GL da leggere.
set -u
BIN="${BIN:-./target/release/zirkio-term}"
DIR="${DIR:-/tmp/riq-ora}"
export NO_AT_BRIDGE=1 GSETTINGS_BACKEND=memory GDK_BACKEND=x11 XDG_SESSION_TYPE=x11
export LANG=C.UTF-8 LC_ALL=C.UTF-8 TERM=xterm-256color
rm -rf "$DIR"; mkdir -p "$DIR"
ZIRKIO_PPM="$DIR/riq.ppm" ZIRKIO_PPM_DOPO=3000 "$BIN" -e python3 "$(dirname "$0")/riquadri.py" >"$DIR/app.log" 2>&1 & APP=$!
sleep 7
kill $APP 2>/dev/null
python3 - "$DIR/riq.ppm" <<'EOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
im.save(sys.argv[1].replace(".ppm", ".png"))
print("colori:", len(im.getcolors(1000000) or []))
EOF
tail -3 "$DIR/app.log"
exit 0
