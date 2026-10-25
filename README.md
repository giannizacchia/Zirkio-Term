# Zirkio Term

**English** · [Italiano](README.it.md)

A terminal for Linux written in Rust, with a native GTK3 interface and OpenGL
text rendering. Built for modest machines: a single codegen unit, full LTO,
stripped binary, no web runtime.

![version](https://img.shields.io/badge/version-0.8.0-blue)

## What it does

- **Tabs and panes**: multiple tabs, horizontal and vertical splits, keyboard
  navigation between panes
- **OpenGL rendering** of the text in a `GtkGLArea`: glyph atlas, system fonts,
  light and dark themes
- **SSH sessions** saved with name and description, listed in the menu and
  opened with `@sh <name>`; key management (push this computer's key)
- **File transfers** in two panes (local and remote) over `scp`, with a
  background queue and whole files or directories
- **Assisted sudo**: recognises the `sudo` password prompt and types it, with a
  choice of whether and where to remember it
- **Remote shutdown** of a saved machine with `@sd <name>`, after confirmation
- **TOML configuration** in `~/.config/zirkio-term/zirkio-term.toml`

## Requirements

- Rust 1.85+ (edition 2024)
- GTK3 with development headers: `gtk3`, `pkg-config`
  - Arch/Manjaro: `sudo pacman -S gtk3 pkgconf`
  - Debian/Ubuntu: `sudo apt install libgtk-3-dev pkg-config`
- Working OpenGL (Mesa is enough)

## Build and run

```bash
cargo build --release
./target/release/zirkio-term
```

The release profile uses `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`
and `strip = true`: slow to compile, small binary.

## Usage

```
zirkio-term [options] [-e command [arguments...]]

  -e, --esegui <cmd>...   run the command instead of the login shell
  -s, --shell <prog>      shell to use (default: the login shell)
      --font <family>     font family
      --size <px>         font size
      --tema <dark|light> terminal theme
      --info              print font, cells and theme without opening a window
  -v, --versione          version
  -h, --aiuto             help
```

## Shortcuts

| Keys | Action |
|---|---|
| `Ctrl+Shift+T` | new tab |
| `Ctrl+Shift+W` | close the pane |
| `Ctrl+Shift+D` | split right |
| `Ctrl+Shift+E` | split down |
| `Ctrl+Shift+arrow` | move between panes |
| `Ctrl+Shift+C / V` | copy / paste |
| `Ctrl+Shift+A` | select all |
| `Ctrl+Shift+ + / - / 0` | font size |
| `F2` | light/dark terminal theme |
| `Shift+PgUp / PgDn` | scroll the scrollback |
| `Ctrl+PgUp / PgDn` | previous / next tab |
| `Ctrl+click` | open a web address |
| `Ctrl+Shift+S` | SSH sessions (list and details) |
| `@sh <name>` | open a saved SSH session |
| `Ctrl+Shift+F` | transfers: two panes, files and directories |
| `@scp <src> <dest>` | copy files or directories to/from a machine |
| `@sd <name>` | shut down a saved machine (asks for confirmation) |
| `F1` | the shortcuts help |

## Structure

```
src/
  main.rs      startup, arguments, shortcuts
  app.rs       window, HeaderBar, tabs, splits, menu
  view.rs      terminal view, selection, copy/paste
  render/      glyph atlas, fonts, OpenGL loader
  term/        parser (vte), performer, input handling
  pty.rs       pseudo-terminal
  ssh.rs       SSH connections
  sshwin.rs    SSH sessions window
  scp.rs       transfers and remote path parsing
  scpwin.rs    two-pane transfers window
  sudo.rs      sudo prompt detection and input
  config.rs    TOML configuration
  prefs.rs     preferences window
tests/         test scripts (GTK, sudo, scp, measurements)
```

## License

MIT — see [LICENSE](LICENSE).
