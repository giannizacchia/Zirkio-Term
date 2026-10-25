#!/bin/bash
# Collaudo grafico senza monitor: avvia il terminale dentro Xvfb e fa uno screenshot.
BIN="${BIN:-./target/debug/zirkio-term}"
OUT="${OUT:-/tmp/shot1.png}"
ARGS=("$@")
if [ ${#ARGS[@]} -eq 0 ]; then
  ARGS=(-e /bin/sh -c 'printf "\033[1;32mzirkio-term\033[0m — prova\n"; ls --color=always /usr/bin | head -25; printf "\n\033[1;34mblu\033[0m \033[1;31mrosso\033[0m \033[4msottolineato\033[0m ─│┌┐└┘ ▏█\n"; sleep 30')
fi
"$BIN" "${ARGS[@]}" >/tmp/zt-app.log 2>&1 &
APP=$!
sleep 6
if ! kill -0 $APP 2>/dev/null; then
  echo "L'APP È MORTA. Log:"
  cat /tmp/zt-app.log
  exit 1
fi
import -window root "$OUT" 2>/tmp/import.log || echo "import fallito: $(cat /tmp/import.log)"
echo "screenshot: $OUT"
kill $APP 2>/dev/null
sleep 1
echo "--- log app ---"
head -30 /tmp/zt-app.log
exit 0
