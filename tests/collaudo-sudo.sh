#!/bin/bash
# Collaudo dell'inserimento automatico della password di sudo, end-to-end e in Xvfb.
#
# Due prove, entrambe in Xvfb, senza sudo vero e senza toccare il portachiavi:
#
#   1. **deve partire**: la «finta sudo» è una copia di /bin/sh chiamata `sudo`,
#      quindi `/proc/<pid>/comm` vale davvero `sudo` e il controllo sul processo in
#      primo piano si collauda per la strada vera;
#   2. **non deve partire**: un prompt *stampato* da uno `sh` qualunque
#      (`printf "[sudo] password for utente: "`) — il testo è identico a quello
#      vero, e solo il controllo su `/proc` impedisce di consegnare la password;
#   3. **sessione SSH**: davanti al terminale c'è `ssh` e la macchina si legge dalla
#      sua riga di comando (`utente@finzione`).  Anche qui la password deve partire,
#      e deve essere quella di *quella* macchina.  La parte `ssh` è simulata con una
#      copia di `/bin/sh` chiamata `ssh` (il vero ssh del 11 è già stato provato a
#      mano: propaga l'eco spento, quindi la guardia sull'eco da sola non basta).
#
# Uso: tests/collaudo-sudo.sh ./target/debug/zirkio-term
set -u
BIN="${1:-./target/debug/zirkio-term}"
MARCHIO="$(date +%H%M%S)"
DISP=":$((80 + RANDOM % 9))"
DIR="/tmp/sudo-collaudo-$MARCHIO"
mkdir -p "$DIR"
SEGRETA="segreta-$MARCHIO"

export DISPLAY="$DISP"
export GDK_BACKEND=x11
export LANG=it_IT.UTF-8
export LC_ALL=it_IT.UTF-8
export ZIRKIO_STATS=1
# Il collaudo non ha un portachiavi: la password si passa dall'ambiente, e questo
# interruttore esiste solo perché il test possa farlo.
export ZIRKIO_SUDO_TEST=1
export ZIRKIO_SUDO_PASSWORD="$SEGRETA"

Xvfb "$DISP" -screen 0 1200x800x24 >/dev/null 2>&1 &
XVFB=$!
sleep 2

# ---------------------------------------------------------------- prova 1: sì
cp /bin/sh "$DIR/sudo"
cat > "$DIR/sudo-finta.sh" <<EOF
printf '[sudo] password for utente: '
stty -echo
IFS= read -r pw
stty echo
printf '\n'
printf 'preso:%s\n' "\$pw" > "$DIR/esito-1"
EOF

# ------------------------------------------------------- prova 3: sessione SSH
cp /bin/sh "$DIR/ssh"
cat > "$DIR/remota.sh" <<EOF
printf '[sudo] password for utente: '
stty -echo
IFS= read -r pw
stty echo
printf '\n'
printf 'preso:%s\n' "\$pw" > "$DIR/esito-3"
EOF

# --------------------------------------------------------------- prova 2: no
cat > "$DIR/solo-testo.sh" <<EOF
printf '[sudo] password for utente: '
IFS= read -r -t 3 preso || preso=NIENTE
printf '%s' "\$preso" > "$DIR/esito-2"
EOF

# Una configurazione per prova: si parte direttamente col comando addosso.
configura() {
    cat > "$DIR/term-$1.toml" <<EOF
scrollback = 2000
padding = 8
shell = "$2"
shell_args = ["-c", "$3"]

[font]
size = 13.0

[window]
width = 900
height = 600

[sudo]
inserimento = true
ricorda = false
EOF
    echo "$DIR/term-$1.toml"
}

configura 1 "$DIR/sudo" ". $DIR/sudo-finta.sh" > /dev/null
configura 2 "/bin/sh" ". $DIR/solo-testo.sh" > /dev/null
# prova 3: argv con la destinazione in coda, come in `ssh … utente@macchina cmd`
cat > "$DIR/term-3.toml" <<EOF
scrollback = 2000
padding = 8
shell = "$DIR/ssh"
shell_args = ["-c", ". $DIR/remota.sh", "utente@finzione"]

[font]
size = 13.0

[window]
width = 900
height = 600

[sudo]
inserimento = true
ricorda = false
EOF

# Istanza unica: se l'app è già aperta, il binario nuovo esce subito e non prova niente.
pkill -x zirkio-term 2>/dev/null
sleep 1

ESITO=0

for n in 1 2 3; do
    export ZIRKIO_TERM_CONFIG="$DIR/term-$n.toml"
    rm -f "$DIR/esito-$n"
    "$BIN" > "$DIR/app-$n.log" 2>&1 &
    APP=$!
    sleep 6
    kill "$APP" 2>/dev/null
    sleep 1
done

echo "--- prova 1: la finta sudo deve ricevere la password"
if [ -s "$DIR/esito-1" ]; then
    letto="$(cat "$DIR/esito-1")"
    if [ "$letto" = "preso:$SEGRETA" ]; then
        echo "OK: la password è arrivata da sola ($letto)"
    else
        echo "KO: la finta sudo ha letto «$letto» invece della password"
        ESITO=1
    fi
else
    echo "KO: la finta sudo non ha ricevuto niente"
    ESITO=1
fi

echo "--- prova 2: un prompt stampato da uno sh qualunque non deve far uscire nulla"
if [ -s "$DIR/esito-2" ]; then
    letto="$(cat "$DIR/esito-2")"
    if [ "$letto" = "NIENTE" ]; then
        echo "OK: nessuna password consegnata a chi ha solo stampato il prompt"
    else
        echo "KO: è stata consegnata la password a uno sh qualunque: «$letto»"
        ESITO=1
    fi
else
    echo "KO: la prova 2 non ha scritto l'esito"
    ESITO=1
fi

echo "--- prova 3: in una sessione SSH, con la macchina letta dalla riga di comando"
if [ -s "$DIR/esito-3" ]; then
    letto="$(cat "$DIR/esito-3")"
    if [ "$letto" = "preso:$SEGRETA" ]; then
        echo "OK: password partita verso la macchina remota ($letto)"
    else
        echo "KO: la sessione remota ha letto «$letto»"
        ESITO=1
    fi
else
    echo "KO: la sessione SSH non ha ricevuto niente"
    ESITO=1
fi

echo "--- registro della prova 1"
grep -E "\[stats\] (sudo|processo)" "$DIR/app-1.log" | tail -10 || true
echo "file della prova: $DIR"
kill "$XVFB" 2>/dev/null
exit "$ESITO"
