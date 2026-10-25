#!/bin/bash
# A/B: con e senza GDK_BACKEND=x11 — la finestra si vede?
set -u
export NO_AT_BRIDGE=1
export GSETTINGS_BACKEND=memory
BIN="${BIN:-./target/release/zirkio-term}"

prova() {
  local label="$1"; shift
  env "$@" "$BIN" >/tmp/ab.log 2>&1 &
  local p=$!
  sleep 6
  local wid
  wid=$(xwininfo -root -tree 2>/dev/null | awk '/0x[0-9a-f]+/ { for (i=1;i<=NF;i++) if ($i ~ /^[0-9]+x[0-9]+\+/) { split($i,d,"x"); if (d[1]+0>best) { best=d[1]+0; id=$1 } } } END { print id }')
  import -window root "/tmp/ab-$label.png" 2>/dev/null
  echo "$label: finestra='${wid:-}' media=$(magick "/tmp/ab-$label.png" -format '%[fx:mean]' info: 2>/dev/null)"
  echo "$label log: $(head -3 /tmp/ab.log | tr '\n' ' ')"
  kill $p 2>/dev/null
  sleep 1
}

prova x11 GDK_BACKEND=x11
prova auto XDG_SESSION_TYPE=
kill 0 2>/dev/null
exit 0
