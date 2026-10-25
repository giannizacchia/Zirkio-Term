#!/bin/bash
# Collaudo dei trasferimenti (`scp`) in Xvfb: un file, una cartella (per la ricorsione),
# un file grande e un ritorno remoto->locale, verso la macchina della sessione «201».
# Uso: tests/collaudo-scp.sh ./target/debug/zirkio-term
# ⚠️ chiede la chiave SSH per utente@192.0.2.11 (BatchMode) e `import` di ImageMagick.
set -u
BIN="${1:-./target/debug/zirkio-term}"
MARCHIO="$(date +%H%M%S)"
DISP=":$((90 + RANDOM % 9))"
export DISPLAY="$DISP"
export GDK_BACKEND=x11
export LANG=it_IT.UTF-8
export LC_ALL=it_IT.UTF-8
export ZIRKIO_STATS=1

Xvfb "$DISP" -screen 0 1400x950x24 >/dev/null 2>&1 &
XVFB=$!
sleep 2

PROVA="/tmp/scp-prova-$MARCHIO"
RITORNO="/tmp/scp-ritorno-$MARCHIO"
mkdir -p "$PROVA/cartella" "$RITORNO"
echo "ciao dal trasferimento $MARCHIO" > "$PROVA/prova.txt"
echo uno > "$PROVA/cartella/a.txt"
echo due > "$PROVA/cartella/b.txt"
head -c 3000000 /dev/urandom > "$PROVA/grosso.bin"

cat > "/tmp/ssh-collaudo-$MARCHIO.toml" <<'EOF'
[[sessione]]
nome = "201"
descrizione = "collaudo"
host = "192.0.2.11"
utente = "utente"
porta = 22
invia_chiave = false
EOF
export ZIRKIO_TERM_SSH="/tmp/ssh-collaudo-$MARCHIO.toml"

# sul remoto: via i file di una prova precedente, e un file da riportare indietro
ssh -o BatchMode=yes utente@192.0.2.11 "rm -f /tmp/prova.txt /tmp/da-201.txt; rm -rf /tmp/cartella; echo indietro > /tmp/da-201.txt"

ZIRKIO_SELFTEST="trasferimenti,scp:$PROVA/prova.txt|201:/tmp/,scp:$PROVA/cartella|201:/tmp/,scp:$PROVA/grosso.bin|201:/tmp/,scp:201:/tmp/da-201.txt|$RITORNO/" \
"$BIN" > "/tmp/collaudo-scp-$MARCHIO.log" 2>&1 &
APP=$!
sleep 16

import -display "$DISP" -window root "/tmp/schermo-scp-$MARCHIO.png" 2>/dev/null

echo "=== registro ==="
grep -E "stats\] (scp|selftest|azione Trasferimenti)" "/tmp/collaudo-scp-$MARCHIO.log" | head -20

echo "=== sul remoto 201 ==="
ssh -o BatchMode=yes utente@192.0.2.11 'ls -l /tmp/prova.txt /tmp/cartella/a.txt /tmp/cartella/b.txt /tmp/grosso.bin'

echo "=== ritorno in $RITORNO ==="
ls -l "$RITORNO"

kill $APP 2>/dev/null
kill $XVFB 2>/dev/null
wait 2>/dev/null
echo "=== screenshot: /tmp/schermo-scp-$MARCHIO.png ==="
