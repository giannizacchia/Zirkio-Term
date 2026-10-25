#!/bin/bash
# Collaudo di `@sd <nome>` in Xvfb: la conferma che nomina la macchina, poi il comando
# di spegnimento nella scheda nuova; e con «Annulla» non deve partire niente.
#
# Nessuna macchina viene spenta: `ssh` è un finto eseguibile che registra gli argomenti
# e il comando remoto si sostituisce con `ZIRKIO_SPEGNI_COMANDO` (interruttore che
# esiste solo per questa prova).
#
# Uso: tests/collaudo-sd.sh ./target/debug/zirkio-term
set -u
BIN="${1:-./target/debug/zirkio-term}"
MARCHIO="$(date +%H%M%S)"
DISP=":$((70 + RANDOM % 9))"
DIR="/tmp/sd-collaudo-$MARCHIO"
mkdir -p "$DIR/percorso" "$DIR/home"
SEGRETA="prova-$MARCHIO"

export DISPLAY="$DISP"
export GDK_BACKEND=x11
export LANG=it_IT.UTF-8
export LC_ALL=it_IT.UTF-8
export ZIRKIO_STATS=1
export ZIRKIO_SPEGNI_COMANDO="comando-di-prova --now"

# Finto `ssh`: scrive gli argomenti ricevuti e resta vivo un secondo.
cat > "$DIR/percorso/ssh" <<EOF
#!/bin/sh
printf '%s\n' "\$*" > "$DIR/ssh-argomenti"
sleep 1
EOF
chmod +x "$DIR/percorso/ssh"
export PATH="$DIR/percorso:$PATH"

# Una macchina salvata che non esiste davvero.
cat > "$DIR/ssh.toml" <<'EOF'
[[sessione]]
nome = "finzione"
descrizione = "collaudo"
host = "192.0.2.13"
utente = "utente"
porta = 22
invia_chiave = false
EOF
export ZIRKIO_TERM_SSH="$DIR/ssh.toml"

cat > "$DIR/term.toml" <<EOF
scrollback = 2000
padding = 8

[window]
width = 900
height = 600
EOF
export ZIRKIO_TERM_CONFIG="$DIR/term.toml"

pkill -x zirkio-term 2>/dev/null
sleep 1
Xvfb "$DISP" -screen 0 1200x800x24 >/dev/null 2>&1 &
XVFB=$!
sleep 2

ESITO=0

# Geometria della finestra piccola (la conferma): X Y L H
geometria_dialogo() {
    local linea
    linea=$(DISPLAY="$DISP" xwininfo -root -tree 2>/dev/null \
        | grep -E '[0-9]+x[0-9]+\+[0-9]+\+[0-9]+' \
        | sed -E 's/.* ([0-9]+x[0-9]+\+[0-9]+\+[0-9]+) .*/\1/' \
        | while read -r geo; do
              L=${geo%%x*}; resto=${geo#*x}; A=${resto%%+*}; resto=${resto#*+}; X=${resto%%+*}; Y=${resto#*+}
              if [ "$L" -lt 600 ]; then echo "$X $Y $L $A"; fi
          done | head -1)
    echo "$linea"
}

# Testo della conferma (per controllo e per trovare i pulsanti)
leggi_dialogo() {
    local x y l a
    read -r x y l a <<<"$(geometria_dialogo)"
    [ -n "${x:-}" ] || return 1
    DISPLAY="$DISP" import -window root "$DIR/schermo-$1.png" 2>/dev/null
    magick "$DIR/schermo-$1.png" -crop "${l}x${a}+${x}+${y}" +repage -resize 250% "$DIR/dlg-$1.png"
    echo "$x $y"
}

# Clic su un pulsante della conferma, coordinate dall'OCR del ritaglio ingrandito
clicca() {
    local nome="$1" x y l a
    read -r x y l a <<<"$(geometria_dialogo)"
    [ -n "${x:-}" ] || { echo "KO: nessuna finestra di conferma"; return 1; }
    DISPLAY="$DISP" import -window root "$DIR/schermo.png" 2>/dev/null
    magick "$DIR/schermo.png" -crop "${l}x${a}+${x}+${y}" +repage -resize 250% "$DIR/dlg.png"
    local coord
    coord=$(tesseract "$DIR/dlg.png" stdout -l afr --psm 11 tsv 2>/dev/null \
        | awk -F"\t" -v chi="$nome" '$12 == chi {printf "%d %d", '"$x"' + ($7 + $9/2)/2.5, '"$y"' + ($8 + $10/2)/2.5; exit}')
    if [ -z "$coord" ]; then
        echo "KO: pulsante «$nome» non trovato (testo: $(tesseract "$DIR/dlg.png" stdout -l afr --psm 6 2>/dev/null | tr '\n' ' '))"
        return 1
    fi
    echo "clic su «$nome» a $coord"
    DISPLAY="$DISP" xdotool mousemove $coord click 1
}

# ---------------------------------------------------------------- prova 1: Spegni
export ZIRKIO_SELFTEST="sd:finzione"
"$BIN" > "$DIR/app-1.log" 2>&1 &
APP=$!
sleep 6

letto=$(leggi_dialogo 1 >/dev/null && tesseract "$DIR/dlg-1.png" stdout -l afr --psm 6 2>/dev/null | tr '\n' ' ')
echo "--- la conferma dice: $letto"
# l'OCR legge le virgolette «» come rumore: si guarda solo il nome
if printf '%s' "$letto" | grep -qi "finzione"; then
    echo "OK: la conferma nomina la macchina"
else
    echo "KO: la conferma non nomina la macchina"
    ESITO=1
fi

if clicca "Spegni"; then
    sleep 4
    if [ -s "$DIR/ssh-argomenti" ]; then
        argomenti="$(cat "$DIR/ssh-argomenti")"
        echo "argomenti di ssh: $argomenti"
        case "$argomenti" in
            *"-t"*"utente@192.0.2.13"*"comando-di-prova --now"*)
                echo "OK: ssh parte con il terminale remoto e il comando di spegnimento" ;;
            *) echo "KO: riga di comando inattesa"; ESITO=1 ;;
        esac
        DISPLAY="$DISP" import -window root "$DIR/2-scheda.png" 2>/dev/null
    else
        echo "KO: dopo la conferma non è partito niente"
        ESITO=1
    fi
else
    ESITO=1
fi
kill "$APP" 2>/dev/null
sleep 2

# --------------------------------------------------------------- prova 2: Annulla
rm -f "$DIR/ssh-argomenti"
"$BIN" > "$DIR/app-2.log" 2>&1 &
APP=$!
sleep 6
if clicca "Annulla"; then
    sleep 3
    if [ -s "$DIR/ssh-argomenti" ]; then
        echo "KO: con «Annulla» è partito lo stesso: $(cat "$DIR/ssh-argomenti")"
        ESITO=1
    else
        echo "OK: con «Annulla» non parte niente"
    fi
else
    ESITO=1
fi
kill "$APP" 2>/dev/null
sleep 1
kill "$XVFB" 2>/dev/null
echo "file della prova: $DIR"
exit "$ESITO"
