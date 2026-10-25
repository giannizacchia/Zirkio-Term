//! Zirkio Terminal — terminale veloce per Linux (GTK3 + OpenGL, Rust).

mod app;
mod config;
mod prefs;
mod pty;
mod render;
mod scp;
mod scpwin;
mod ssh;
mod sshwin;
mod sudo;
mod term;
mod view;

use std::cell::RefCell;
use std::process::ExitCode;
use std::rc::Rc;

use gtk::prelude::*;

use config::Config;

/// Le scorciatoie, elencate nella finestra «Scorciatoie» (F1).
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+Maiusc+T", "nuova scheda"),
    ("Ctrl+Maiusc+W", "chiudi il pannello"),
    ("Ctrl+Maiusc+D", "dividi a destra"),
    ("Ctrl+Maiusc+E", "dividi in basso"),
    ("Ctrl+Maiusc+freccia", "spostati fra i pannelli"),
    ("Ctrl+Maiusc+C / V", "copia / incolla"),
    ("Ctrl+Maiusc+A", "seleziona tutto"),
    ("Ctrl+Maiusc+ + / - / 0", "dimensione del carattere"),
    ("F2", "tema chiaro/scuro del terminale"),
    ("Maiusc+PgSu / PgGiù", "scorri la cronologia"),
    ("Ctrl+PgSu / PgGiù", "scheda precedente / successiva"),
    ("Ctrl+clic", "apre un indirizzo web"),
    ("Ctrl+Maiusc+S", "sessioni SSH (elenco e dati)"),
    ("@sh <nome>", "apre una sessione SSH salvata"),
    ("Ctrl+Maiusc+F", "trasferimenti: due riquadri, file e cartelle"),
    ("@scp <sorg> <dest>", "copia file o cartelle da/verso una macchina"),
    ("@sd <nome>", "spegne una macchina salvata (chiede conferma)"),
    ("F1", "questa guida"),
];

const USAGE: &str = "\
zirkio-term — terminale veloce per Linux (GTK3)

USO:
    zirkio-term [opzioni] [-e comando [argomenti...]]

OPZIONI:
    -e, --esegui <cmd>...   esegue il comando invece della shell di login
    -s, --shell <prog>      shell da usare (default: quella di login)
        --font <famiglia>   famiglia di caratteri (default: monospazio di sistema)
        --size <px>         dimensione del carattere
        --tema <dark|light> tema del terminale
        --info              stampa font, celle e tema senza aprire la finestra
    -v, --versione          versione
    -h, --aiuto             questo testo

FILE DI CONFIGURAZIONE:
    ~/.config/zirkio-term/zirkio-term.toml
";

thread_local! {
    /// Tiene vivo lo stato dell'app finché vive la finestra.
    static APP: RefCell<Option<Rc<app::App>>> = const { RefCell::new(None) };
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zirkio-term: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut cfg = Config::load()?;
    let mut info_only = false;
    let mut analizza: Option<String> = None;
    let mut command: Option<Vec<String>> = None;

    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "-h" | "--aiuto" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            "-v" | "--versione" | "--version" => {
                println!("zirkio-term {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--info" => info_only = true,
            "--analizza" => {
                i += 1;
                analizza = Some(argv.get(i).cloned().ok_or("manca il file per --analizza")?);
            }
            "-s" | "--shell" => {
                i += 1;
                cfg.shell = Some(argv.get(i).cloned().ok_or("manca il valore di --shell")?);
            }
            "--font" => {
                i += 1;
                cfg.font.family = Some(argv.get(i).cloned().ok_or("manca il valore di --font")?);
            }
            "--size" => {
                i += 1;
                let v: f32 = argv
                    .get(i)
                    .ok_or("manca il valore di --size")?
                    .parse()
                    .map_err(|_| "--size vuole un numero")?;
                cfg.font.size = v.clamp(6.0, 72.0);
            }
            "--tema" | "--theme" => {
                i += 1;
                let name = argv.get(i).cloned().ok_or("manca il valore di --tema")?;
                cfg.theme = if name.to_ascii_lowercase().starts_with('l') {
                    config::Theme::light()
                } else {
                    config::Theme::dark()
                };
            }
            "-e" | "--esegui" | "--exec" => {
                let rest: Vec<String> = argv[i + 1..].to_vec();
                if rest.is_empty() {
                    return Err("-e vuole un comando".into());
                }
                command = Some(rest);
                break;
            }
            other => return Err(format!("opzione sconosciuta: {other} (vedi --aiuto)")),
        }
        i += 1;
    }

    if let Some(cmd) = command {
        cfg.shell = Some(cmd[0].clone());
        cfg.shell_args = cmd[1..].to_vec();
    }

    // Strumento di analisi: `--analizza <file>` rimette un flusso di byte nel motore
    // e dice cosa finisce nella griglia (utile per i difetti di resa segnalati).
    if let Some(path) = analizza {
        let data = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
        let mut term = term::Terminal::new(
            100,
            30,
            cfg.scrollback,
            cfg.theme.palette256(),
            cfg.theme.fg,
            cfg.theme.bg,
        );
        let mut parser = vte::Parser::new();
        parser.advance(&mut term, &data);
        let mut riquadri = 0;
        let mut lettere_acs = 0;
        let acs = "qxlkmjntuvw";
        for line in &term.lines {
            for c in line {
                if ('\u{2500}'..='\u{257f}').contains(&c.ch) {
                    riquadri += 1;
                } else if acs.contains(c.ch) {
                    lettere_acs += 1;
                }
            }
        }
        println!("byte: {}", data.len());
        println!("celle con riquadri (U+2500-U+257F): {riquadri}");
        println!("celle con lettere ACS grezze (q x l k m j n t u v w): {lettere_acs}");
        // Caratteri "strani" (fuori ASCII stampabile) con il loro conteggio: serve a
        // capire *quali* caratteri il programma manda e noi non disegniamo bene.
        let mut conta: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
        for line in &term.lines {
            for c in line {
                if !(' '..='~').contains(&c.ch) {
                    *conta.entry(c.ch).or_insert(0) += 1;
                }
            }
        }
        let mut elenco: Vec<(char, usize)> = conta.into_iter().collect();
        elenco.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        println!("caratteri non ASCII nella griglia:");
        for (c, n) in elenco.iter().take(30) {
            println!("  U+{:04X} {:?} × {n}", *c as u32, c);
        }
        let righe = term.lines.len().min(40);
        let testo: String = term.lines.iter().take(righe).map(|l| l.iter().map(|c| c.ch).collect::<String>()).collect::<Vec<_>>().join("\n");
        println!("prime {righe} righe:\n{testo}");
        return Ok(());
    }

    if info_only {
        let fonts = render::font::FontSet::load(
            cfg.font.family.as_deref(),
            cfg.font.size,
            cfg.font.line_height,
            cfg.font.bold_strength,
        )?;
        println!("zirkio-term {}", env!("CARGO_PKG_VERSION"));
        println!("{}", fonts.info());
        println!("tema: {}", cfg.theme.name);
        println!("scrollback: {} righe", cfg.scrollback);
        let (shell, args) = cfg.shell_command();
        println!("shell: {shell} {}", args.join(" "));
        return Ok(());
    }

    let application = gtk::Application::new(Some("org.zirkio.term"), gtk::gio::ApplicationFlags::default());
    application.connect_activate(move |gtk_app| {
        if std::env::var_os("ZIRKIO_STATS").is_some() {
            eprintln!("[stats] activate: finestre={}", gtk_app.windows().len());
        }
        if let Some(window) = gtk_app.windows().first() {
            window.present();
            return;
        }
        match app::App::new(gtk_app, cfg.clone()) {
            Ok(app) => {
                APP.with(|slot| *slot.borrow_mut() = Some(app));
            }
            Err(e) => {
                eprintln!("zirkio-term: {e}");
                gtk_app.quit();
            }
        }
    });
    // Gli argomenti li abbiamo già letti noi: a GTK non se ne passa nessuno.
    application.run_with_args(&["zirkio-term"]);
    Ok(())
}
