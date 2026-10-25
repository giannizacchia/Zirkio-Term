#!/bin/bash
# Misure: tempo di avvio (fino alla finestra) e CPU da fermo.
# Uso: xvfb-run -a bash tests/misure.sh
set -u
BIN="${BIN:-./target/release/zirkio-term}"

start=$(date +%s%N)
"$BIN" >/tmp/misure-app.log 2>&1 &
PID=$!
xdotool search --sync --pid $PID >/dev/null 2>&1
end=$(date +%s%N)
echo "avvio (exec -> finestra): $(( (end - start) / 1000000 )) ms"

ticks() { awk '{print $14 + $15}' "/proc/$1/stat"; }
sleep 2
t0=$(ticks $PID)
sleep 10
t1=$(ticks $PID)
HZ=$(getconf CLK_TCK)
CPU=$(awk -v a="$t0" -v b="$t1" -v hz="$HZ" 'BEGIN{printf "%.1f", (b-a)/hz/10*100}')
echo "CPU da fermo (10 s): ${CPU}% di un core"
RSS=$(awk '/VmRSS/{print $2}' "/proc/$PID/status")
echo "memoria: ${RSS} kB"

# Ridisegno continuo: 2 secondi di output fitto
xdotool type --delay 0 'seq 1 20000'
xdotool key Return
sleep 3
t2=$(ticks $PID)
sleep 3
t3=$(ticks $PID)
CPU2=$(awk -v a="$t2" -v b="$t3" -v hz="$HZ" 'BEGIN{printf "%.1f", (b-a)/hz/3*100}')
echo "CPU mentre scorre output (3 s): ${CPU2}% di un core"

kill $PID 2>/dev/null
wait $PID 2>/dev/null
head -5 /tmp/misure-app.log
exit 0
