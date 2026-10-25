#!/bin/bash
# Che nome/classe ha la finestra GTK? (per farci puntare xdotool nei collaudi)
set -u
export NO_AT_BRIDGE=1
BIN="${BIN:-./target/release/zirkio-term}"
"$BIN" >/tmp/g.log 2>&1 &
P=$!
sleep 5
echo "== per nome 'zirkio':"
xdotool search --name zirkio 2>&1 | head -3
echo "== per classe (tutte):"
for w in $(xdotool search --class . 2>/dev/null | head -8); do
  echo "  $w  name=$(xdotool getwindowname $w 2>/dev/null)  class=$(xprop -id $w WM_CLASS 2>/dev/null)"
done
echo "== per pid:"
xdotool search --pid $P 2>&1 | head -3
echo "== visibili per nome (tutte):"
xdotool search --onlyvisible --name . 2>&1 | head -5
kill $P 2>/dev/null
exit 0
