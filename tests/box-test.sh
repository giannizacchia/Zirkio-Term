#!/bin/sh
# Disegno delle linee: charset DEC (come mc) e UTF-8 diretto.
printf '\033[0m-- ACS (ESC(0), come usa mc) --\n'
printf '\033(0lqqqqqqqk\033(B\n'
printf '\033(0x       x\033(B\n'
printf '\033(0mqqqqqqqj\033(B\n'
printf '\033(0tqqqqqwu\033(B\n'
printf '\033(0x     x x\033(B\n'
printf '\033(0mqqqqqvj\033(B\n'
printf '\n-- UTF-8 diretto --\n'
printf '─ │ ┌ ┐ └ ┘ ├ ┤ ┬ ┴ ┼ █ ▒ ░\n'
printf '\n-- simboli --\n'
printf '⏵ ⇅ · ✓ ≥ ± ° … → ← ↑ ↓\n'
sleep 90
