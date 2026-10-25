# Zirkio Term

**Italiano** · [English](README.md)

Terminale per Linux scritto in Rust, con interfaccia GTK3 nativa e rendering del
testo in OpenGL. Pensato per macchine modeste: un solo codegen unit, LTO pieno,
binario spogliato, nessun runtime web.

![versione](https://img.shields.io/badge/versione-0.8.0-blue)

## Cosa fa

- **Schede e pannelli**: schede multiple, divisione orizzontale e verticale,
  spostamento fra i pannelli da tastiera
- **Rendering OpenGL** del testo in una `GtkGLArea`: atlante di glifi, font di
  sistema, temi chiaro e scuro
- **Sessioni SSH** salvate con nome e descrizione, elenco nel menu e apertura
  con `@sh <nome>`; gestione delle chiavi (invio della chiave del computer)
- **Trasferimenti file** a due riquadri (locale e remoto) via `scp`, con coda in
  background e file o cartelle intere
- **Sudo assistito**: riconosce la richiesta di password di `sudo` e la inserisce,
  con scelta se ricordarla e dove
- **Spegnimento remoto** di una macchina salvata con `@sd <nome>`, previa conferma
- **Configurazione TOML** in `~/.config/zirkio-term/zirkio-term.toml`

## Requisiti

- Rust 1.85+ (edition 2024)
- GTK3 con header di sviluppo: `gtk3`, `pkg-config`
  - Arch/Manjaro: `sudo pacman -S gtk3 pkgconf`
  - Debian/Ubuntu: `sudo apt install libgtk-3-dev pkg-config`
- OpenGL funzionante (Mesa basta)

## Compilare ed eseguire

```bash
cargo build --release
./target/release/zirkio-term
```

Il profilo release usa `lto = "fat"`, `codegen-units = 1`, `panic = "abort"` e
`strip = true`: la compilazione e' lenta, il binario piccolo.

## Uso

```
zirkio-term [opzioni] [-e comando [argomenti...]]

  -e, --esegui <cmd>...   esegue il comando invece della shell di login
  -s, --shell <prog>      shell da usare (default: quella di login)
      --font <famiglia>   famiglia di caratteri
      --size <px>         dimensione del carattere
      --tema <dark|light> tema del terminale
      --info              stampa font, celle e tema senza aprire la finestra
  -v, --versione          versione
  -h, --aiuto             aiuto
```

## Scorciatoie

| Tasti | Azione |
|---|---|
| `Ctrl+Maiusc+T` | nuova scheda |
| `Ctrl+Maiusc+W` | chiudi il pannello |
| `Ctrl+Maiusc+D` | dividi a destra |
| `Ctrl+Maiusc+E` | dividi in basso |
| `Ctrl+Maiusc+freccia` | spostati fra i pannelli |
| `Ctrl+Maiusc+C / V` | copia / incolla |
| `Ctrl+Maiusc+A` | seleziona tutto |
| `Ctrl+Maiusc+ + / - / 0` | dimensione del carattere |
| `F2` | tema chiaro/scuro del terminale |
| `Maiusc+PgSu / PgGiù` | scorri la cronologia |
| `Ctrl+PgSu / PgGiù` | scheda precedente / successiva |
| `Ctrl+clic` | apre un indirizzo web |
| `Ctrl+Maiusc+S` | sessioni SSH (elenco e dati) |
| `@sh <nome>` | apre una sessione SSH salvata |
| `Ctrl+Maiusc+F` | trasferimenti: due riquadri, file e cartelle |
| `@scp <sorg> <dest>` | copia file o cartelle da/verso una macchina |
| `@sd <nome>` | spegne una macchina salvata (chiede conferma) |
| `F1` | guida delle scorciatoie |

## Struttura

```
src/
  main.rs      avvio, argomenti, scorciatoie
  app.rs       finestra, HeaderBar, schede, divisioni, menu
  view.rs      vista del terminale, selezione, copia/incolla
  render/      atlante dei glifi, font, caricamento OpenGL
  term/        parser (vte), performer, gestione input
  pty.rs       pseudo-terminale
  ssh.rs       connessioni SSH
  sshwin.rs    finestra delle sessioni SSH
  scp.rs       trasferimenti e parsing dei percorsi remoti
  scpwin.rs    finestra dei trasferimenti a due riquadri
  sudo.rs      riconoscimento del prompt di sudo e inserimento
  config.rs    configurazione TOML
  prefs.rs     finestra delle preferenze
```

## Licenza

MIT — vedi [LICENSE](LICENSE).
