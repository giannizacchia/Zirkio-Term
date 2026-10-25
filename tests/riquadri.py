#!/usr/bin/env python3
"""Riquadri di prova (UTF-8 garantito): doppie, singole, spesse, incroci, blocchi."""
import time

print("\033[0mdoppie:")
print("\u2554" + "\u2550" * 6 + "\u2557")
print("\u2551" + "  x   " + "\u2551")
print("\u255a" + "\u2550" * 6 + "\u255d")
print("singole:")
print("\u250c" + "\u2500" * 6 + "\u2510")
print("\u2502" + "  x   " + "\u2502")
print("\u2514" + "\u2500" * 6 + "\u2518")
print("spesse:")
print("\u250f" + "\u2501" * 6 + "\u2513")
print("\u2503" + "  x   " + "\u2503")
print("\u2517" + "\u2501" * 6 + "\u251b")
print("incroci: " + "\u256c\u253c\u2560\u2550\u2563\u2551\u256b\u2551")
print("blocchi: " + "\u2588\u2588\u2593\u2593\u2592\u2592\u2591\u2591")
time.sleep(60)
