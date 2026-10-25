//! Finestra **Trasferimenti**: due riquadri (questo computer / macchina remota) e una
//! coda di `scp` con avanzamento, come in `mc` — ma senza dover ricordare la sintassi.
//!
//! Si apre con `Ctrl+Maiusc+F`, dal menu SSH → «Trasferimenti file…», oppure da sola
//! quando si digita `@scp …` nel terminale (in quel caso il trasferimento è già in coda).
//!
//! Regole di casa rispettate:
//! - il thread grafico non fa **mai** rete né attese: sonde e `scp` girano in thread e
//!   mandano eventi su un canale, raccolti da un `timeout_add_local` (nessun `Send`
//!   sulle chiusure GTK);
//! - niente `borrow_mut` mentre si chiamano funzioni che rientrano nei `RefCell`
//!   (i panico da `RefCell` con `panic = "abort"` chiudono la finestra);
//! - la password non si salva: `scp` parte con `BatchMode=yes` e usa la chiave; se la
//!   chiave non c'è l'errore lo dice (e si installa da «Sessioni SSH»).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use gtk::prelude::*;

use crate::app::App;
use crate::scp::{self, Info, Lato, Tipo};
use crate::ssh::Sessione;

thread_local! {
    static FINESTRA: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// Stile degli elenchi: cartelle in blu, riga selezionata in blu pieno con testo bianco
/// (come `mc`), intestazioni di colonna piatte e cliccabili.
///
/// I colori stanno **nel CSS** e non nel markup: un `foreground` scritto nel markup
/// vince sul CSS e la riga selezionata resterebbe blu su blu, illeggibile.
fn installa_stile_elenchi() {
    use std::sync::Once;
    static UNA: Once = Once::new();
    UNA.call_once(|| {
        let prov = gtk::CssProvider::new();
        let _ = prov.load_from_data(
            b".nome-cartella { color: #3584e4; font-weight: bold; }\n\
              .scelta { background-color: #3584e4; }\n\
              .scelta label { color: #ffffff; }\n\
              button.testata { padding: 0 3px; min-height: 0; min-width: 0;\n\
                background: none; border: none; box-shadow: none; }\n\
              button.testata label { font-size: smaller; opacity: 0.75; }",
        );
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &prov,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    });
}

/// Per che cosa è ordinato un elenco.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Campo {
    Nome,
    Tipo,
    Dimensione,
}

/// Ordina le voci: `..` sempre in cima, **cartelle prima dei file**, poi il campo scelto
/// (e a parità di campo, il nome).
fn ordina_per(voci: &mut [Voce], campo: Campo, crescente: bool) {
    voci.sort_by(|a, b| {
        if a.nome == ".." {
            return std::cmp::Ordering::Less;
        }
        if b.nome == ".." {
            return std::cmp::Ordering::Greater;
        }
        let gruppo = b.cartella.cmp(&a.cartella);
        if gruppo != std::cmp::Ordering::Equal {
            return gruppo;
        }
        let nome_a = a.nome.to_lowercase();
        let nome_b = b.nome.to_lowercase();
        let ordine = match campo {
            Campo::Nome => nome_a.cmp(&nome_b),
            Campo::Tipo => tipo_file(&a.nome, a.cartella)
                .cmp(&tipo_file(&b.nome, b.cartella))
                .then_with(|| nome_a.cmp(&nome_b)),
            Campo::Dimensione => {
                // le cartelle non hanno una dimensione da mostrare («—»): valgono 0, così
                // restano in ordine alfabetico invece di seguire la dimensione dell'inode
                let byte_a = if a.cartella { 0 } else { a.byte };
                let byte_b = if b.cartella { 0 } else { b.byte };
                byte_a.cmp(&byte_b).then_with(|| nome_a.cmp(&nome_b))
            }
        };
        if crescente { ordine } else { ordine.reverse() }
    });
}

/// Larghezza (px) della colonna «Tipo» e di quella «Dimensione» negli elenchi.
const LARG_TIPO: i32 = 150;
const LARG_DIM: i32 = 95;

/// Che cosa è un file, dedotto **dall'estensione**.
///
/// Non si usa `gio::content_type_guess` per il nome: passandogli un buffer di dati vuoto
/// glib risponde `application/x-zerosize` («documento vuoto») e ignora l'estensione —
/// un `.png` risultava «documento vuoto».  Una tabella esplicita è prevedibile, veloce e
/// in italiano.
fn tipo_file(nome: &str, cartella: bool) -> String {
    if cartella {
        return "cartella".to_string();
    }
    let minuscolo = nome.to_lowercase();
    // il punto in **prima** posizione (`.bashrc`) non è un'estensione
    let estensione = match minuscolo.rfind('.') {
        Some(0) | None => None,
        Some(i) => Some(&minuscolo[i + 1..]),
    };
    // estensioni **composte** prima di quelle semplici (`.pkg.tar.zst` è un pacchetto,
    // non un archivio qualunque)
    for (suffisso, tipo) in [
        (".pkg.tar.zst", "pacchetto"),
        (".pkg.tar.xz", "pacchetto"),
        (".tar.gz", "archivio"),
        (".tar.xz", "archivio"),
        (".tar.zst", "archivio"),
        (".tar.bz2", "archivio"),
        (".tar", "archivio"),
    ] {
        if minuscolo.ends_with(suffisso) {
            return tipo.to_string();
        }
    }
    let Some(e) = estensione else {
        return "file".to_string();
    };
    let tipo = match e {
        "txt" | "md" | "log" | "nfo" | "rst" => "testo",
        "pdf" => "PDF",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "heic" | "tiff" | "tif" | "raw" => "immagine",
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma" => "audio",
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "mpg" | "mpeg" => "video",
        "zip" | "tar" | "gz" | "xz" | "bz2" | "zst" | "7z" | "rar" | "lz" | "lz4" => "archivio",
        "iso" | "img" => "immagine disco",
        "sh" | "bash" | "zsh" | "fish" | "ps1" => "script",
        "rs" => "Rust",
        "py" | "pyw" => "Python",
        "go" => "Go",
        "c" | "h" => "C",
        "cpp" | "cc" | "hpp" | "cxx" => "C++",
        "js" | "mjs" => "JavaScript",
        "ts" => "TypeScript",
        "java" => "Java",
        "kt" => "Kotlin",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "ini" | "conf" | "cfg" | "env" => "configurazione",
        "xml" => "XML",
        "html" | "htm" => "pagina web",
        "css" => "foglio di stile",
        "csv" => "CSV",
        "odt" | "odp" | "ods" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "rtf" => "documento",
        "deb" | "rpm" | "apk" | "exe" | "msi" | "appimage" => "pacchetto",
        "ttf" | "otf" | "woff" | "woff2" => "carattere",
        "pub" => "chiave pubblica",
        "db" | "sqlite" | "sql" => "database",
        "torrent" => "torrent",
        "desktop" => "avviatore",
        "xml.gz" | "bak" | "old" | "swp" => "file",
        "bin" | "dat" | "img_raw" => "dati",
        altro => return altro.to_uppercase(),
    };
    tipo.to_string()
}

/// Riga di intestazione delle colonne: ogni titolo è un pulsante che **ordina** per
/// quella colonna (un secondo clic inverte il verso).  Restituisce anche le etichette,
/// che servono a disegnare la freccia ▲/▼ sulla colonna in uso.
type Intestazione = (Campo, &'static str, gtk::Button, gtk::Label);

fn bottone_testata(
    voci: &mut Vec<Intestazione>,
    testo: &'static str,
    campo: Campo,
    larghezza: i32,
    a_destra: bool,
) -> gtk::Button {
    let etichetta = gtk::Label::new(Some(testo));
    etichetta.set_xalign(if a_destra { 1.0 } else { 0.0 });
    let pulsante = gtk::Button::new();
    pulsante.add(&etichetta);
    pulsante.set_relief(gtk::ReliefStyle::None);
    pulsante.set_focus_on_click(false);
    pulsante.style_context().add_class("testata");
    pulsante.set_tooltip_text(Some(&format!("ordina per «{testo}»")));
    if larghezza > 0 {
        pulsante.set_size_request(larghezza, -1);
    }
    voci.push((campo, testo, pulsante.clone(), etichetta));
    pulsante
}

fn testata_colonne() -> (gtk::Box, Vec<Intestazione>) {
    let t = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    t.set_border_width(4);
    let spazio = gtk::Label::new(None);
    spazio.set_size_request(14, -1);
    t.pack_start(&spazio, false, false, 0);

    let mut voci: Vec<Intestazione> = Vec::new();
    let p_nome = bottone_testata(&mut voci, "Nome", Campo::Nome, 0, false);
    p_nome.set_hexpand(true);
    t.pack_start(&p_nome, true, true, 0);
    let p_tipo = bottone_testata(&mut voci, "Tipo", Campo::Tipo, LARG_TIPO, false);
    t.pack_start(&p_tipo, false, false, 0);
    let p_dim = bottone_testata(&mut voci, "Dimensione", Campo::Dimensione, LARG_DIM, true);
    t.pack_end(&p_dim, false, false, 0);
    (t, voci)
}

/// Una riga di un elenco (file o cartella).
#[derive(Clone)]
struct Voce {
    nome: String,
    cartella: bool,
    byte: u64,
}

enum Evento {
    Elenco {
        remoto: bool,
        generazione: u64,
        esito: Result<(String, Vec<Voce>), String>,
    },
    Info {
        id: u64,
        esito: Result<Info, String>,
    },
    Avanzamento {
        id: u64,
        pct: u8,
        riga: String,
    },
    Avviso {
        id: u64,
        testo: String,
    },
    Fine {
        id: u64,
        esito: Result<(), String>,
    },
}

struct Riquadro {
    /// Titolo della colonna («Questo computer»); `None` quando la testa della colonna
    /// è occupata dal selettore della macchina.
    titolo: Option<gtk::Label>,
    remoto: RefCell<Option<Sessione>>,
    percorso: RefCell<String>,
    voci: RefCell<Vec<Voce>>,
    generazione: Cell<u64>,
    listbox: gtk::ListBox,
    barra: gtk::Entry,
    stato: gtk::Label,
    nascosti: gtk::CheckButton,
    n_voci: Cell<usize>,
    n_cartelle: Cell<usize>,
    /// Ordinamento in corso e verso.
    campo: Cell<Campo>,
    crescente: Cell<bool>,
    /// Le tre intestazioni cliccabili (campo, nome, pulsante, etichetta).
    intestazioni: RefCell<Vec<(Campo, &'static str, gtk::Button, gtk::Label)>>,
    /// Indici delle voci **selezionate**: la selezione la teniamo noi (vedi `alterna`),
    /// non `GtkListBox`, così un clic aggiunge/toglie senza bisogno di Ctrl.
    scelti: RefCell<std::collections::HashSet<usize>>,
    /// Canale verso il ciclo degli eventi (serve a `apri_voce` per ricaricare).
    tx: Sender<Evento>,
}

struct Riga {
    barra: gtk::ProgressBar,
    stato: gtk::Label,
    info: gtk::Label,
    /// «Annulla»: spento appena il trasferimento è finito (non ha più senso).
    annulla: gtk::Button,
}

struct Compito {
    id: u64,
    sorg: Lato,
    dest: Lato,
    annulla: Arc<AtomicBool>,
}

pub(crate) struct Ui {
    finestra: gtk::Window,
    locale: Rc<Riquadro>,
    remoto: Rc<Riquadro>,
    coda: gtk::ListBox,
    righe: RefCell<HashMap<u64, Riga>>,
    compiti: RefCell<VecDeque<Compito>>,
    in_corso: Cell<bool>,
    prossimo_id: Cell<u64>,
    tx: Sender<Evento>,
    attivo: Cell<bool>,
}

// ------------------------------------------------------------------ file system

fn elenco_locale(percorso: &str, nascosti: bool) -> Result<(String, Vec<Voce>), String> {
    let p = Path::new(percorso);
    let pwd = std::fs::canonicalize(p)
        .map(|c| c.to_string_lossy().to_string())
        .unwrap_or_else(|_| percorso.to_string());
    let mut voci = Vec::new();
    let lettura = std::fs::read_dir(&pwd).map_err(|e| format!("{pwd}: {e}"))?;
    for voce in lettura.flatten() {
        let nome = voce.file_name().to_string_lossy().to_string();
        if !nascosti && nome.starts_with('.') {
            continue;
        }
        let md = match voce.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        voci.push(Voce { nome, cartella: md.is_dir(), byte: md.len() });
    }
    if let Some(parent) = Path::new(&pwd).parent() {
        if parent != Path::new(&pwd) {
            voci.push(Voce { nome: "..".into(), cartella: true, byte: 0 });
        }
    }
    ordina(&mut voci);
    Ok((pwd, voci))
}

/// Elenco di una cartella remota con **una sola** connessione: `cd` (che risolve `~`) e
/// `find -printf` con tipo e dimensione, così non si interpreta l'output di `ls`.
fn elenco_remoto(s: &Sessione, percorso: &str) -> Result<(String, Vec<Voce>), String> {
    let qp = q_remoto(percorso);
    let cmd = format!(
        "cd {qp} 2>/dev/null || {{ echo '__NOREPERTO__'; exit 1; }}; pwd; \
         LC_ALL=C find . -mindepth 1 -maxdepth 1 -printf '%y\\t%s\\t%f\\n' 2>/dev/null"
    );
    let (prog, args) = s.ssh_comando(&cmd, true);
    let out = Command::new(&prog)
        .args(&args)
        .output()
        .map_err(|e| format!("ssh: {e}"))?;
    let testo = String::from_utf8_lossy(&out.stdout);
    let mut righe = testo.lines();
    let pwd = righe.next().unwrap_or("").trim().to_string();
    if pwd == "__NOREPERTO__" || pwd.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("«{}»: cartella «{percorso}» non accessibile", s.nome)
        } else {
            err
        });
    }
    let mut voci = Vec::new();
    for riga in righe {
        let mut campi = riga.splitn(3, '\t');
        let (Some(tipo), Some(byte), Some(nome)) = (campi.next(), campi.next(), campi.next()) else {
            continue;
        };
        if nome.is_empty() {
            continue;
        }
        voci.push(Voce {
            nome: nome.to_string(),
            cartella: tipo == "d",
            byte: byte.parse().unwrap_or(0),
        });
    }
    if let Some(parent) = Path::new(&pwd).parent() {
        if parent != Path::new(&pwd) {
            voci.push(Voce { nome: "..".into(), cartella: true, byte: 0 });
        }
    }
    ordina(&mut voci);
    Ok((pwd, voci))
}

/// Cartelle prima, poi per nome (senza badare a maiuscole).
fn ordina(voci: &mut [Voce]) {
    voci.sort_by(|a, b| {
        if a.nome == ".." {
            return std::cmp::Ordering::Less;
        }
        if b.nome == ".." {
            return std::cmp::Ordering::Greater;
        }
        b.cartella
            .cmp(&a.cartella)
            .then_with(|| a.nome.to_lowercase().cmp(&b.nome.to_lowercase()))
    });
}

/// Quoting sul remoto che lascia espandere `~` (`$HOME`).
fn q_remoto(p: &str) -> String {
    match p.strip_prefix('~') {
        Some(resto) => format!("$HOME'{}'", resto.replace('\'', r"'\''")),
        None => format!("'{}'", p.replace('\'', r"'\''")),
    }
}

/// Unisce una cartella e un nome, senza raddoppiare le barre.
/// Chiede un nome con una finestrella modale; `fatto` riceve il nome già validato.
fn chiedi_nome(
    radice: Option<gtk::Window>,
    titolo: &str,
    domanda: &str,
    fatto: Rc<dyn Fn(String)>,
) {
    let dialogo = gtk::Dialog::with_buttons(
        Some(titolo),
        radice.as_ref(),
        gtk::DialogFlags::MODAL,
        &[
            ("Annulla", gtk::ResponseType::Cancel),
            ("Crea", gtk::ResponseType::Ok),
        ],
    );
    dialogo.set_default_response(gtk::ResponseType::Ok);
    let area = dialogo.content_area();
    area.set_spacing(6);
    area.set_border_width(10);
    let etichetta = gtk::Label::new(Some(domanda));
    etichetta.set_xalign(0.0);
    area.pack_start(&etichetta, false, false, 0);
    let campo = gtk::Entry::new();
    campo.set_activates_default(true);
    campo.set_placeholder_text(Some("es. documenti"));
    area.pack_start(&campo, false, false, 0);
    let errore = gtk::Label::new(None);
    errore.set_xalign(0.0);
    area.pack_start(&errore, false, false, 0);

    let c2 = campo.clone();
    let e2 = errore.clone();
    dialogo.connect_response(move |d, risposta| {
        if risposta != gtk::ResponseType::Ok {
            d.close();
            return;
        }
        match nome_valido(&c2.text()) {
            Ok(nome) => {
                d.close();
                fatto(nome);
            }
            Err(msg) => e2.set_markup(&format!("<span foreground=\"#e01b24\">{msg}</span>")),
        }
    });
    dialogo.show_all();
    campo.grab_focus();
}

/// Il nome di una nuova cartella deve essere un nome solo (niente percorsi).
fn nome_valido(grezzo: &str) -> Result<String, &'static str> {
    let nome = grezzo.trim();
    if nome.is_empty() {
        return Err("scrivi un nome");
    }
    if nome == "." || nome == ".." {
        return Err("«.» e «..» esistono già");
    }
    if nome.contains('/') {
        return Err("il nome non può contenere «/»");
    }
    Ok(nome.to_string())
}

/// Crea una cartella **nella cartella corrente** del riquadro (locale o sulla macchina),
/// poi ricarica l'elenco.  La rete sta nel thread, mai nel filo dell'interfaccia.
fn crea_cartella(r: &Rc<Riquadro>, tx: &Sender<Evento>, nome: &str) {
    let percorso = r.percorso.borrow().clone();
    let completo = unisci(&percorso, nome);
    let remoto = r.remoto.borrow().clone();
    let nascosti = r.nascosti.is_active();
    let generazione = r.generazione.get() + 1;
    r.generazione.set(generazione);
    r.stato.set_text(&format!("creo «{nome}»…"));
    let e_remoto = remoto.is_some();
    let tx = tx.clone();
    let nome = nome.to_string();
    std::thread::spawn(move || {
        let creazione = match &remoto {
            Some(s) => {
                let (prog, args) = s.ssh_comando(&format!("mkdir -- {}", q_remoto(&completo)), true);
                match Command::new(&prog).args(&args).output() {
                    Err(e) => Err(format!("ssh: {e}")),
                    Ok(out) if !out.status.success() => {
                        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
                    }
                    Ok(_) => Ok(()),
                }
            }
            None => std::fs::create_dir(&completo).map_err(|e| e.to_string()),
        };
        let esito = match creazione {
            Err(e) if e.is_empty() => Err(format!("non ho potuto creare «{nome}»")),
            Err(e) => Err(format!("non ho potuto creare «{nome}»: {e}")),
            Ok(()) => match &remoto {
                Some(s) => elenco_remoto(s, &percorso),
                None => elenco_locale(&percorso, nascosti),
            },
        };
        let _ = tx.send(Evento::Elenco { remoto: e_remoto, generazione, esito });
    });
}

fn unisci(base: &str, nome: &str) -> String {
    if nome.is_empty() {
        return base.to_string();
    }
    if base.ends_with('/') {
        format!("{base}{nome}")
    } else {
        format!("{base}/{nome}")
    }
}

// ------------------------------------------------------------------ riquadro

impl Riquadro {
    fn lato(&self, nome: &str) -> Lato {
        let base = self.percorso.borrow().clone();
        Lato {
            remoto: self.remoto.borrow().clone(),
            percorso: unisci(&base, nome),
        }
    }

    /// Ricarica l'elenco (locale in linea, remoto in un thread).
    fn carica(&self, tx: &Sender<Evento>) {
        let generazione = self.generazione.get() + 1;
        self.generazione.set(generazione);
        let percorso = self.percorso.borrow().clone();
        self.stato.set_text("carico…");
        match self.remoto.borrow().clone() {
            Some(s) => {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let esito = elenco_remoto(&s, &percorso);
                    let _ = tx.send(Evento::Elenco { remoto: true, generazione, esito });
                });
            }
            None => {
                let nascosti = self.nascosti.is_active();
                let esito = elenco_locale(&percorso, nascosti);
                let _ = tx.send(Evento::Elenco { remoto: false, generazione, esito });
            }
        }
    }

    /// Scrive il titolo della colonna con la macchina in uso.
    fn aggiorna_titolo(&self, base: &str) {
        let Some(titolo) = &self.titolo else { return };
        let testo = match self.remoto.borrow().as_ref() {
            Some(s) if s.descrizione.trim().is_empty() => format!("{base} — {}", s.nome),
            Some(s) => format!("{base} — {} · {}", s.nome, s.descrizione),
            None => base.to_string(),
        };
        titolo.set_markup(&format!("<b>{}</b>", gtk::glib::markup_escape_text(&testo)));
    }

    /// Vero se questa risposta riguarda ancora il riquadro (non è superata).
    fn accetta(&self, generazione: u64) -> bool {
        generazione == self.generazione.get()
    }

    fn riempi(&self, pwd: String, voci: Vec<Voce>) {
        *self.percorso.borrow_mut() = pwd.clone();
        self.barra.set_text(&pwd);
        *self.voci.borrow_mut() = voci;
        self.disegna();
    }

    /// Costruisce le righe dell'elenco nell'ordine scelto (l'ordinamento è **locale**:
    /// cambiare colonna non tocca la rete).
    fn disegna(&self) {
        self.scelti.borrow_mut().clear();
        for w in self.listbox.children() {
            self.listbox.remove(&w);
        }
        let mut voci = self.voci.borrow().clone();
        ordina_per(&mut voci, self.campo.get(), self.crescente.get());
        *self.voci.borrow_mut() = voci.clone();

        for (i, v) in voci.iter().enumerate() {
            let riga = gtk::ListBoxRow::new();
            riga.set_widget_name(&i.to_string());
            let box_riga = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            box_riga.set_border_width(4);
            box_riga.pack_start(&icona(v.cartella), false, false, 0);

            // Nome: il colore sta nella **classe CSS** (blu per le cartelle), non nel
            // markup: così la riga selezionata può diventare bianca su fondo blu.
            let nome = gtk::Label::new(Some(&v.nome));
            nome.set_xalign(0.0);
            nome.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            if v.cartella {
                nome.style_context().add_class("nome-cartella");
            }
            box_riga.pack_start(&nome, true, true, 0);

            // colonna «Tipo», in MAIUSCOLO
            let tipo = tipo_file(&v.nome, v.cartella).to_uppercase();
            let etichetta_tipo = gtk::Label::new(None);
            etichetta_tipo.set_markup(&format!(
                "<span size=\"small\">{}</span>",
                gtk::glib::markup_escape_text(&tipo)
            ));
            etichetta_tipo.set_xalign(0.0);
            etichetta_tipo.set_ellipsize(gtk::pango::EllipsizeMode::End);
            etichetta_tipo.set_size_request(LARG_TIPO, -1);
            etichetta_tipo.set_width_chars(1);
            etichetta_tipo.set_tooltip_text(Some(&tipo));
            box_riga.pack_start(&etichetta_tipo, false, false, 0);

            // colonna «Dimensione»: B / kB / MB / GB / TB secondo la grandezza
            let misura = gtk::Label::new(Some(&if v.cartella {
                "—".to_string()
            } else {
                scp::formatta_byte(v.byte)
            }));
            misura.set_xalign(1.0);
            misura.set_size_request(LARG_DIM, -1);
            box_riga.pack_end(&misura, false, false, 0);

            riga.add(&box_riga);
            self.listbox.add(&riga);
            riga.show_all();
        }
        self.listbox.show_all();
        self.n_voci.set(voci.iter().filter(|v| v.nome != "..").count());
        self.n_cartelle
            .set(voci.iter().filter(|v| v.cartella && v.nome != "..").count());
        self.aggiorna_stato(0);
    }

    /// Aggiunge o toglie una riga dalla selezione (un clic = un interruttore).
    fn alterna(&self, indice: usize, riga: &gtk::ListBoxRow) {
        let adesso_scelta = {
            let mut scelti = self.scelti.borrow_mut();
            if scelti.contains(&indice) {
                scelti.remove(&indice);
                false
            } else {
                scelti.insert(indice);
                true
            }
        };
        if adesso_scelta {
            riga.style_context().add_class("scelta");
        } else {
            riga.style_context().remove_class("scelta");
        }
        self.aggiorna_stato(self.scelti.borrow().len());
    }

    /// Toglie tutte le righe dalla selezione.
    fn svuota_selezione(&self) {
        self.scelti.borrow_mut().clear();
        for i in 0..self.listbox.children().len() {
            if let Some(riga) = self.listbox.row_at_index(i as i32) {
                riga.style_context().remove_class("scelta");
            }
        }
        self.aggiorna_stato(0);
    }

    /// Ordina per `campo`; se è già quello, inverte il verso.
    fn cambia_ordine(&self, campo: Campo) {
        if self.campo.get() == campo {
            self.crescente.set(!self.crescente.get());
        } else {
            self.campo.set(campo);
            self.crescente.set(true);
        }
        self.mostra_frecce();
        self.disegna();
    }

    /// Freccia ▲/▼ sull'intestazione della colonna in uso.
    fn mostra_frecce(&self) {
        let campo = self.campo.get();
        let freccia = if self.crescente.get() { " ▲" } else { " ▼" };
        for (c, testo, _, etichetta) in self.intestazioni.borrow().iter() {
            etichetta.set_text(&if *c == campo {
                format!("{testo}{freccia}")
            } else {
                testo.to_string()
            });
        }
    }

    /// Riga di stato: quante voci, quante cartelle e — se c'è — quante selezionate.
    fn aggiorna_stato(&self, selezionati: usize) {
        let mut testo = format!("{} voci · {} cartelle", self.n_voci.get(), self.n_cartelle.get());
        match selezionati {
            0 => {}
            1 => testo.push_str(" · 1 selezionato"),
            n => testo.push_str(&format!(" · {n} selezionati")),
        }
        self.stato.set_text(&testo);
    }

    fn errore(&self, msg: &str) {
        self.stato.set_text(&format!("errore: {msg}"));
    }
}

/// Il tema di icone ha questo nome? (per scegliere un disegno o ripiegare sul testo)
fn tema_ha_icona(nome: &str) -> bool {
    gdk::Screen::default()
        .and_then(|s| gtk::IconTheme::for_screen(&s))
        .is_some_and(|t| t.has_icon(nome))
}

fn icona(cartella: bool) -> gtk::Image {
    let preferiti: &[&str] = if cartella {
        &["folder-symbolic", "folder", "inode-directory-symbolic"]
    } else {
        &["text-x-generic-symbolic", "text-x-generic", "document-new-symbolic"]
    };
    let tema = gdk::Screen::default().and_then(|s| gtk::IconTheme::for_screen(&s));
    for nome in preferiti {
        if tema.as_ref().is_some_and(|t| t.has_icon(nome)) {
            return gtk::Image::from_icon_name(Some(nome), gtk::IconSize::Menu);
        }
    }
    gtk::Image::from_icon_name(Some("text-x-generic"), gtk::IconSize::Menu)
}

/// Costruisce un riquadro (elenco + barra del percorso) e la sua colonna.
fn costruisci_riquadro(
    titolo: &str,
    remoto: Option<Sessione>,
    tx: &Sender<Evento>,
    testa_extra: Option<&gtk::Widget>,
    finestra: &gtk::Window,
) -> (Rc<Riquadro>, gtk::Box) {
    let colonna = gtk::Box::new(gtk::Orientation::Vertical, 4);
    colonna.set_border_width(6);

    let testa = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    // Con `testa_extra` (la tendina della macchina) il titolo scritto non serve: la prima
    // riga della colonna è il selettore stesso, largo quanto la colonna.
    let etichetta = if testa_extra.is_some() {
        None
    } else {
        let e = gtk::Label::new(None);
        e.set_markup(&format!("<b>{}</b>", gtk::glib::markup_escape_text(titolo)));
        e.set_xalign(0.0);
        testa.pack_start(&e, false, false, 0);
        Some(e)
    };
    if let Some(extra) = testa_extra {
        testa.pack_start(extra, true, true, 0);
    }

    let su = gtk::Button::with_label("↑");
    su.set_tooltip_text(Some("Cartella superiore"));
    let casa = gtk::Button::with_label("~");
    casa.set_tooltip_text(Some("Cartella personale"));
    testa.pack_end(&casa, false, false, 0);
    testa.pack_end(&su, false, false, 0);
    colonna.pack_start(&testa, false, false, 0);

    // riga del percorso: casella + pulsante «nuova cartella» a destra
    let riga_percorso = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let barra = gtk::Entry::new();
    barra.set_text(&remoto.as_ref().map(|_| "~".to_string()).unwrap_or_else(|| {
        std::env::var("HOME").unwrap_or_else(|_| "/".into())
    }));
    barra.set_hexpand(true);
    riga_percorso.pack_start(&barra, true, true, 0);
    // Crea una cartella **nella cartella corrente** di questo riquadro: locale o remota
    // secondo la macchina del riquadro (chiesto dall'utente il 25/10).
    let nuova = gtk::Button::new();
    if tema_ha_icona("folder-new-symbolic") {
        nuova.set_image(Some(&gtk::Image::from_icon_name(
            Some("folder-new-symbolic"),
            gtk::IconSize::Menu,
        )));
    } else {
        nuova.set_label("+ dir");
    }
    nuova.set_tooltip_text(Some("Nuova cartella qui"));
    nuova.set_relief(gtk::ReliefStyle::None);
    nuova.set_focus_on_click(false);
    riga_percorso.pack_start(&nuova, false, false, 0);
    colonna.pack_start(&riga_percorso, false, false, 0);

    let (barra_colonne, intestazioni) = testata_colonne();
    colonna.pack_start(&barra_colonne, false, false, 0);

    let listbox = gtk::ListBox::new();
    // La selezione la gestiamo noi (`SelectionMode::None`): un clic aggiunge o toglie la
    // riga, senza Ctrl (chiesto dall'utente il 25/10).  Un clic da solo non invia niente.
    listbox.set_selection_mode(gtk::SelectionMode::None);
    listbox.set_activate_on_single_click(false);
    let scorr = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scorr.set_policy(gtk::PolicyType::Automatic, gtk::PolicyType::Automatic);
    scorr.set_min_content_height(140);
    scorr.add(&listbox);
    colonna.pack_start(&scorr, true, true, 0);

    let piede = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let stato = gtk::Label::new(None);
    stato.set_xalign(0.0);
    stato.set_ellipsize(gtk::pango::EllipsizeMode::End);
    piede.pack_start(&stato, true, true, 0);
    let nascosti = gtk::CheckButton::with_label("nascosti");
    nascosti.set_active(true);
    nascosti.set_tooltip_text(Some("Mostra i file che cominciano con il punto"));
    piede.pack_end(&nascosti, false, false, 0);
    colonna.pack_start(&piede, false, false, 0);

    let percorso = barra.text().to_string();
    let r = Rc::new(Riquadro {
        titolo: etichetta.clone(),
        remoto: RefCell::new(remoto),
        percorso: RefCell::new(percorso),
        voci: RefCell::new(Vec::new()),
        generazione: Cell::new(0),
        listbox: listbox.clone(),
        barra: barra.clone(),
        stato: stato.clone(),
        nascosti: nascosti.clone(),
        n_voci: Cell::new(0),
        n_cartelle: Cell::new(0),
        campo: Cell::new(Campo::Nome),
        crescente: Cell::new(true),
        intestazioni: RefCell::new(intestazioni),
        scelti: RefCell::new(std::collections::HashSet::new()),
        tx: tx.clone(),
    });

    {
        // Invio nella barra del percorso: vai a quella cartella.
        let r2 = r.clone();
        let tx = tx.clone();
        barra.connect_activate(move |b| {
            let nuovo = b.text().trim().to_string();
            if nuovo.is_empty() {
                return;
            }
            *r2.percorso.borrow_mut() = nuovo;
            r2.carica(&tx);
        });
    }
    {
        let r2 = r.clone();
        let tx = tx.clone();
        su.connect_clicked(move |_| {
            let p = r2.percorso.borrow().clone();
            let padre = Path::new(&p)
                .parent()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or(p);
            *r2.percorso.borrow_mut() = padre;
            r2.carica(&tx);
        });
    }
    {
        let r2 = r.clone();
        let tx = tx.clone();
        casa.connect_clicked(move |_| {
            let casa = std::env::var("HOME").unwrap_or_else(|_| "/".into());
            *r2.percorso.borrow_mut() = casa;
            r2.carica(&tx);
        });
    }
    {
        let r2 = r.clone();
        let tx = tx.clone();
        nascosti.connect_toggled(move |_| r2.carica(&tx));
    }
    {
        // Clic singolo: **aggiunge o toglie** la voce dalla selezione (come `mc`), senza
        // Ctrl.  Doppio clic: entra nella cartella.  Nessuno dei due invia file.
        let r2 = r.clone();
        listbox.connect_button_press_event(move |lb, ev| {
            let Some(riga) = lb.row_at_y(ev.position().1 as i32) else {
                return glib::Propagation::Proceed;
            };
            let Ok(indice) = riga.widget_name().as_str().parse::<usize>() else {
                return glib::Propagation::Proceed;
            };
            if ev.event_type() == gdk::EventType::DoubleButtonPress {
                r2.svuota_selezione();
                apri_voce(&r2, &riga);
            } else {
                r2.alterna(indice, &riga);
            }
            glib::Propagation::Proceed
        });
    }
    {
        let r2 = r.clone();
        let tx2 = tx.clone();
        let finestra2 = finestra.clone();
        nuova.connect_clicked(move |_| {
            let radice = Some(finestra2.clone());
            let r3 = r2.clone();
            let tx3 = tx2.clone();
            chiedi_nome(
                radice,
                "Nuova cartella",
                "Nome della cartella:",
                Rc::new(move |nome: String| crea_cartella(&r3, &tx3, &nome)),
            );
        });
    }
    {
        // clic su un'intestazione: ordina per quella colonna, o inverte il verso
        let pulsanti: Vec<(Campo, gtk::Button)> = r
            .intestazioni
            .borrow()
            .iter()
            .map(|(campo, _, p, _)| (*campo, p.clone()))
            .collect();
        for (campo, pulsante) in pulsanti {
            let r2 = r.clone();
            pulsante.connect_clicked(move |_| r2.cambia_ordine(campo));
        }
        r.mostra_frecce();
    }
    (r, colonna)
}

// ------------------------------------------------------------------ coda

fn riga_coda(ui: &Rc<Ui>, c: &Compito) -> (gtk::ListBoxRow, Riga) {
    let riga = gtk::ListBoxRow::new();
    riga.set_activatable(false);
    riga.set_widget_name(&c.id.to_string());
    let box_riga = gtk::Box::new(gtk::Orientation::Vertical, 3);
    box_riga.set_border_width(5);

    let alto = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let titolo = gtk::Label::new(None);
    titolo.set_markup(&format!(
        "<b>{}</b>",
        gtk::glib::markup_escape_text(&format!(
            "{} → {}",
            c.sorg.etichetta(),
            c.dest.etichetta()
        ))
    ));
    titolo.set_xalign(0.0);
    titolo.set_ellipsize(gtk::pango::EllipsizeMode::End);
    alto.pack_start(&titolo, true, true, 0);

    let annulla = gtk::Button::with_label("Annulla");
    annulla.set_relief(gtk::ReliefStyle::None);
    {
        let flag = c.annulla.clone();
        let id = c.id;
        let ui2 = ui.clone();
        annulla.connect_clicked(move |b| {
            flag.store(true, Ordering::Relaxed);
            b.set_sensitive(false);
            b.set_label("annullo…");
            if let Some(r) = ui2.righe.borrow().get(&id) {
                r.stato.set_text("annullamento richiesto…");
            }
        });
    }
    alto.pack_end(&annulla, false, false, 0);
    box_riga.pack_start(&alto, false, false, 0);

    let barra = gtk::ProgressBar::new();
    barra.set_show_text(true);
    barra.set_text(Some("analisi…"));
    box_riga.pack_start(&barra, false, false, 0);

    let info = gtk::Label::new(None);
    info.set_xalign(0.0);
    info.set_ellipsize(gtk::pango::EllipsizeMode::End);
    info.set_text("guardo che cosa c'è…");
    box_riga.pack_start(&info, false, false, 0);

    let stato = gtk::Label::new(None);
    stato.set_xalign(0.0);
    stato.set_ellipsize(gtk::pango::EllipsizeMode::End);
    box_riga.pack_start(&stato, false, false, 0);

    riga.add(&box_riga);
    riga.show_all();
    let r = Riga { barra, stato, info, annulla };
    (riga, r)
}

/// Esegue `scp` in una pty (per leggere l'avanzamento) e manda gli eventi.
fn esegui_scp(
    id: u64,
    sorg: Lato,
    dest: Lato,
    annulla: Arc<AtomicBool>,
    tx: Sender<Evento>,
) {
    let info: Info = match scp::rileva(&sorg) {
        Ok(i) => i,
        Err(e) => {
            let _ = tx.send(Evento::Info { id, esito: Err(e) });
            return;
        }
    };
    if let Some(avviso) = scp::avviso_opzioni(&sorg, &dest) {
        let _ = tx.send(Evento::Avviso { id, testo: avviso });
    }
    let _ = tx.send(Evento::Info { id, esito: Ok(info) });

    let (prog, args) = scp::comando(&sorg, &dest, info.tipo == Tipo::Cartella);
    if std::env::var_os("ZIRKIO_STATS").is_some() {
        eprintln!("[stats] scp: {prog} {}", args.join(" "));
    }

    // Ultime righe di `scp`: servono a spiegare un errore.
    let ultime: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
    let ultime_sink = ultime.clone();
    let tx_dati = tx.clone();
    let buffer = RefCell::new(String::new());
    let sink = crate::pty::Sink {
        on_data: Box::new(move |dati: Vec<u8>| {
            let mut buf = buffer.borrow_mut();
            buf.push_str(&String::from_utf8_lossy(&dati));
            loop {
                let Some(pos) = buf.find(['\r', '\n']) else { break };
                let riga: String = buf.drain(..=pos).collect();
                let riga = riga.trim().to_string();
                if riga.is_empty() {
                    continue;
                }
                if let Some(pct) = scp::percentuale(&riga) {
                    let _ = tx_dati.send(Evento::Avanzamento { id, pct, riga: riga.clone() });
                }
                if let Ok(mut u) = ultime_sink.lock() {
                    if !riga.ends_with('%') {
                        u.push_back(riga);
                        while u.len() > 6 {
                            u.pop_front();
                        }
                    }
                }
            }
            if buf.len() > 8192 {
                buf.clear();
            }
        }),
        on_exit: Box::new(|| {}),
    };

    let esito = match crate::pty::Pty::spawn(&prog, &args, 100, 30, None, sink) {
        Ok(mut pty) => loop {
            if annulla.load(Ordering::Relaxed) {
                pty.kill();
                break Err("annullato".to_string());
            }
            match pty.try_wait() {
                Ok(Some(codice)) if codice == 0 => break Ok(()),
                Ok(Some(codice)) => {
                    let coda = ultime
                        .lock()
                        .map(|u| u.iter().cloned().collect::<Vec<_>>().join(" · "))
                        .unwrap_or_default();
                    break Err(spiega(codice, &coda));
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(120)),
                Err(e) => break Err(format!("scp: {e}")),
            }
        },
        Err(e) => Err(format!("non riesco ad avviare scp: {e}")),
    };
    let _ = tx.send(Evento::Fine { id, esito });
}

/// Traduce l'uscita di `scp` in una frase utile (con l'ultima riga del suo messaggio).
fn spiega(codice: i32, coda: &str) -> String {
    let base = match codice {
        255 => "connessione rifiutata, oppure chiave non installata: apri «Sessioni SSH» \
                (Ctrl+Maiusc+S) e attiva «Invia la chiave di questo computer»"
            .to_string(),
        1 => "scp: errore (il file esiste? hai i permessi?)".to_string(),
        _ => format!("scp è uscito con codice {codice}"),
    };
    if coda.trim().is_empty() {
        base
    } else {
        format!("{base} — {coda}")
    }
}

// ------------------------------------------------------------------ eventi

fn avvia_prossimo(ui: &Rc<Ui>) {
    if ui.in_corso.get() {
        return;
    }
    let Some(compito) = ui.compiti.borrow_mut().pop_front() else {
        return;
    };
    ui.in_corso.set(true);
    let tx = ui.tx.clone();
    let id = compito.id;
    std::thread::spawn(move || {
        esegui_scp(
            id,
            compito.sorg.clone(),
            compito.dest.clone(),
            compito.annulla.clone(),
            tx,
        );
    });
}

fn accoda_lati(ui: &Rc<Ui>, sorg: Lato, dest: Lato) -> u64 {
    let id = ui.prossimo_id.get();
    ui.prossimo_id.set(id + 1);
    let compito = Compito { id, sorg, dest, annulla: Arc::new(AtomicBool::new(false)) };
    let (riga, vista) = riga_coda(ui, &compito);
    ui.coda.add(&riga);
    ui.coda.show_all();
    ui.righe.borrow_mut().insert(id, vista);
    ui.compiti.borrow_mut().push_back(compito);
    avvia_prossimo(ui);
    id
}

fn gestisci(ui: &Rc<Ui>, evento: Evento) {
    match evento {
        Evento::Elenco { remoto, generazione, esito } => {
            let r = if remoto { ui.remoto.clone() } else { ui.locale.clone() };
            if !r.accetta(generazione) {
                return;
            }
            match esito {
                Ok((pwd, voci)) => r.riempi(pwd, voci),
                Err(e) => r.errore(&e),
            }
        }
        Evento::Info { id, esito } => match esito {
            Ok(info) => {
                if let Some(r) = ui.righe.borrow().get(&id) {
                    r.info.set_text(&info.descrizione());
                    r.barra.set_text(Some("in corso"));
                }
            }
            Err(e) => {
                // il trasferimento non è nemmeno partito
                if let Some(r) = ui.righe.borrow().get(&id) {
                    r.barra.set_text(Some("errore"));
                    r.barra.set_fraction(0.0);
                    r.annulla.set_sensitive(false);
                    r.stato.set_markup(&format!(
                        "<span foreground=\"#c01c28\">{}</span>",
                        gtk::glib::markup_escape_text(&e)
                    ));
                }
                ui.in_corso.set(false);
                avvia_prossimo(ui);
            }
        },
        Evento::Avanzamento { id, pct, riga } => {
            if let Some(r) = ui.righe.borrow().get(&id) {
                r.barra.set_fraction(pct as f64 / 100.0);
                r.barra.set_text(Some(&format!("{pct}%")));
                r.stato.set_text(&riga);
            }
        }
        Evento::Avviso { id, testo } => {
            if let Some(r) = ui.righe.borrow().get(&id) {
                r.info.set_text(&testo);
            }
        }
        Evento::Fine { id, esito } => {
            if let Some(r) = ui.righe.borrow().get(&id) {
                r.annulla.set_sensitive(false);
                match &esito {
                    Ok(()) => {
                        r.barra.set_fraction(1.0);
                        r.barra.set_text(Some("fatto"));
                        r.stato.set_markup(&format!(
                            "<span foreground=\"#26a269\">trasferimento completato</span>"
                        ));
                    }
                    Err(e) => {
                        r.barra.set_text(Some("errore"));
                        r.stato.set_markup(&format!(
                            "<span foreground=\"#c01c28\">{}</span>",
                            gtk::glib::markup_escape_text(e)
                        ));
                    }
                }
            }
            ui.in_corso.set(false);
            avvia_prossimo(ui);
        }
    }
}

// ------------------------------------------------------------------ finestra

/// Apre la finestra (o la riporta davanti se c'è già).
pub fn apri(app: &Rc<App>) -> Rc<Ui> {
    if let Some(ui) = FINESTRA.with(|f| f.borrow().clone()) {
        ui.finestra.present();
        return ui;
    }

    installa_stile_elenchi();
    let finestra = gtk::Window::new(gtk::WindowType::Toplevel);
    finestra.set_title("Trasferimenti — Zirkio Terminal");
    finestra.set_transient_for(Some(&app.window));
    finestra.set_default_size(1060, 760);
    let intestazione = gtk::HeaderBar::new();
    intestazione.set_title(Some("Trasferimenti"));
    intestazione.set_subtitle(Some("file e cartelle fra questo computer e una macchina"));
    // Senza questo la finestra non ha la × nell'angolo e non si chiude (segnalato
    // dall'utente il 25/10): una `GtkWindow` con HeaderBar non mostra i pulsanti da sola.
    intestazione.set_show_close_button(true);
    finestra.set_deletable(true);
    finestra.set_titlebar(Some(&intestazione));

    let (tx, rx) = mpsc::channel::<Evento>();

    // tendina delle sessioni, nella colonna di destra
    let sessioni = app.sessioni();
    let scelta = gtk::ComboBoxText::new();
    for s in &sessioni {
        let etichetta = if s.descrizione.trim().is_empty() {
            s.nome.clone()
        } else {
            format!("{} · {}", s.nome, s.descrizione)
        };
        scelta.append(Some(&s.nome), &etichetta);
    }
    if sessioni.is_empty() {
        scelta.append_text("(nessuna sessione salvata — Ctrl+Maiusc+S)");
    }

    let prima = sessioni.first().cloned();

    // La scelta della macchina sta **in cima alla colonna di destra**, quella con i file
    // della macchina: è lì che la si cerca (richiesta dell'utente del 25/10).  Occupa la
    // riga del titolo e si allarga con la colonna; è una `ComboBoxText` vera.
    let testa_scelta = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    testa_scelta.set_valign(gtk::Align::Center);
    let etichetta_scelta = gtk::Label::new(None);
    etichetta_scelta.set_markup("<b>Macchina:</b>");
    testa_scelta.pack_start(&etichetta_scelta, false, false, 0);
    scelta.set_hexpand(true);
    scelta.set_size_request(150, -1);
    scelta.set_tooltip_text(Some("Macchina su cui lavorare (le sessioni di Ctrl+Maiusc+S)"));
    testa_scelta.pack_start(&scelta, true, true, 0);
    if sessioni.is_empty() {
        scelta.set_sensitive(false);
    } else if let Some(nome) = prima.as_ref().map(|s| s.nome.clone()) {
        scelta.set_active_id(Some(&nome));
    }

    let (locale, colonna_locale) =
        costruisci_riquadro("Questo computer", None, &tx, None, &finestra);
    let (remoto, colonna_remota) = costruisci_riquadro(
        "Macchina remota",
        prima.clone(),
        &tx,
        Some(testa_scelta.clone().upcast_ref::<gtk::Widget>()),
        &finestra,
    );

    let divisione = gtk::Paned::new(gtk::Orientation::Horizontal);
    divisione.set_position(480);
    divisione.pack1(&colonna_locale, true, true);
    divisione.pack2(&colonna_remota, true, true);

    // pulsanti di copia: uno per verso, sotto ogni riquadro
    let copia_destra = gtk::Button::with_label("Copia nella macchina →");
    copia_destra.set_tooltip_text(Some("Copia il file selezionato a sinistra nella cartella a destra"));
    let copia_sinistra = gtk::Button::with_label("← Copia in questo computer");
    copia_sinistra.set_tooltip_text(Some("Copia il file selezionato a destra nella cartella a sinistra"));
    let riga_destra = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    riga_destra.pack_start(&copia_destra, false, false, 0);
    colonna_locale.pack_start(&riga_destra, false, false, 0);
    let riga_sinistra = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    riga_sinistra.pack_start(&copia_sinistra, false, false, 0);
    colonna_remota.pack_start(&riga_sinistra, false, false, 0);

    // coda dei trasferimenti
    let coda = gtk::ListBox::new();
    coda.set_selection_mode(gtk::SelectionMode::None);
    let scorr_coda = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scorr_coda.set_min_content_height(150);
    scorr_coda.add(&coda);
    let riquadro_coda = gtk::Frame::new(Some("Trasferimenti"));
    riquadro_coda.add(&scorr_coda);

    let sopra = gtk::Paned::new(gtk::Orientation::Vertical);
    // Posizione **calcolata sull'altezza vera**: con un valore fisso, su una finestra
    // più bassa la parte alta veniva compressa e la testa spariva.
    {
        let gia = Cell::new(false);
        sopra.connect_size_allocate(move |p, alloc| {
            let h = alloc.height();
            if !gia.get() && h > 240 {
                gia.set(true);
                let pos = ((h as f64 * 0.62) as i32).clamp(200, h - 145);
                p.set_position(pos);
            }
        });
    }
    sopra.pack1(&divisione, true, true);
    sopra.pack2(&riquadro_coda, false, false);

    finestra.add(&sopra);

    let ui = Rc::new(Ui {
        finestra: finestra.clone(),
        locale: locale.clone(),
        remoto: remoto.clone(),
        coda: coda.clone(),
        righe: RefCell::new(HashMap::new()),
        compiti: RefCell::new(VecDeque::new()),
        in_corso: Cell::new(false),
        prossimo_id: Cell::new(1),
        tx: tx.clone(),
        attivo: Cell::new(true),
    });

    // (il clic e il doppio clic sono gestiti dentro `costruisci_riquadro`)
    // --- pulsanti di copia
    {
        let ui2 = ui.clone();
        copia_destra.connect_clicked(move |_| copia_selezionato(&ui2, &ui2.locale, &ui2.remoto));
    }
    {
        let ui2 = ui.clone();
        copia_sinistra.connect_clicked(move |_| copia_selezionato(&ui2, &ui2.remoto, &ui2.locale));
    }
    // --- cambio macchina
    {
        let ui2 = ui.clone();
        let sessioni = sessioni.clone();
        let tx = tx.clone();
        scelta.connect_changed(move |c| {
            let Some(id) = c.active_id() else { return };
            let id = id.to_string();
            let Some(s) = sessioni.iter().find(|s| s.nome == id).cloned() else { return };
            *ui2.remoto.remoto.borrow_mut() = Some(s);
            *ui2.remoto.percorso.borrow_mut() = "~".to_string();
            ui2.remoto.barra.set_text("~");
            ui2.remoto.carica(&tx);
        });
    }
    // --- raccolta eventi (un solo ciclo, sul thread grafico)
    {
        let ui2 = ui.clone();
        let rx = Rc::new(RefCell::new(rx));
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
            if !ui2.attivo.get() {
                return gtk::glib::ControlFlow::Break;
            }
            loop {
                let evento = { rx.borrow_mut().try_recv() };
                match evento {
                    Ok(e) => gestisci(&ui2, e),
                    Err(_) => break,
                }
            }
            gtk::glib::ControlFlow::Continue
        });
    }
    {
        // Esc chiude (la × dell'HeaderBar c'è, ma Esc è la via rapida).
        let finestra2 = finestra.clone();
        finestra.connect_key_press_event(move |_, ev| {
            if *ev.keyval() == 0xff1b {
                // 0xff1b = keysym X11 di Esc (stessa convenzione del resto del codice)
                finestra2.close();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
    }
    {
        let attivo = ui.attivo.clone();
        finestra.connect_destroy(move |_| {
            attivo.set(false);
            FINESTRA.with(|f| *f.borrow_mut() = None);
        });
    }
    FINESTRA.with(|f| *f.borrow_mut() = Some(ui.clone()));

    locale.aggiorna_titolo("Questo computer");
    finestra.show_all();
    if std::env::var_os("ZIRKIO_STATS").is_some() {
        eprintln!(
            "[stats] trasferimenti: finestra mostrata (chiudibile={}, × nell'HeaderBar)",
            finestra.is_deletable()
        );
    }
    locale.carica(&tx);
    if prima.is_some() {
        remoto.carica(&tx);
    }
    ui
}

/// Doppio clic / Invio su una voce: cartella → ci si entra, file → si copia dall'altra parte.
fn apri_voce(da: &Rc<Riquadro>, riga: &gtk::ListBoxRow) {
    let Ok(indice) = riga.widget_name().as_str().parse::<usize>() else { return };
    let voce = {
        let voci = da.voci.borrow();
        match voci.get(indice) {
            Some(v) => v.clone(),
            None => return,
        }
    };
    if voce.nome == ".." {
        let p = da.percorso.borrow().clone();
        let padre = Path::new(&p)
            .parent()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or(p);
        *da.percorso.borrow_mut() = padre;
        da.carica(&da.tx.clone());
        return;
    }
    if voce.cartella {
        let nuovo = unisci(&da.percorso.borrow().clone(), &voce.nome);
        *da.percorso.borrow_mut() = nuovo;
        da.carica(&da.tx.clone());
        return;
    }
    // Un file: il doppio clic non fa niente, resta selezionato.  Mandarlo subito era il
    // difetto: bastava sfiorarlo e partiva il trasferimento (segnalato il 25/10).
}

/// Copia **tutti i file e le cartelle selezionati** da un riquadro all'altro, nella
/// cartella corrente della destinazione.  Uno per uno, così ognuno ha la sua riga di
/// avanzamento nella coda.
fn copia_selezionato(ui: &Rc<Ui>, da: &Rc<Riquadro>, a: &Rc<Riquadro>) {
    if da.scelti.borrow().is_empty() {
        da.stato.set_text("seleziona prima uno o più file (un clic per aggiungerli)");
        return;
    }
    if a.remoto.borrow().is_none() && da.remoto.borrow().is_none() {
        da.stato.set_text("scegli una macchina remota");
        return;
    }
    // I nomi si leggono **prima** di accodare: `accoda_lati` tocca l'interfaccia e non
    // va lasciato un prestito aperto sui `RefCell` del riquadro.
    let mut indici: Vec<usize> = da.scelti.borrow().iter().copied().collect();
    indici.sort_unstable();
    let mut nomi: Vec<String> = Vec::new();
    {
        let voci = da.voci.borrow();
        for indice in indici {
            if let Some(v) = voci.get(indice) {
                if v.nome != ".." {
                    nomi.push(v.nome.clone());
                }
            }
        }
    }
    if nomi.is_empty() {
        da.stato.set_text("la selezione non contiene file da copiare");
        return;
    }
    let dest = a.lato("");
    let quanti = nomi.len();
    for nome in nomi {
        let sorg = da.lato(&nome);
        accoda_lati(ui, sorg, dest.clone());
    }
    da.stato.set_text(&format!("{quanti} in coda verso «{}»", dest.etichetta()));
    da.svuota_selezione();
}

/// Ingresso da `@scp <sorgente> <destinazione>`: mette in coda e apre la finestra.
pub fn accoda(app: &Rc<App>, sorg_spec: &str, dest_spec: &str, cwd: Option<&Path>) -> Result<u64, String> {
    let sessioni = app.sessioni();
    let sorg = scp::analizza(sorg_spec, &sessioni, cwd)?;
    let dest = scp::analizza(dest_spec, &sessioni, cwd)?;
    if sorg.remoto.is_none() && dest.remoto.is_none() {
        return Err("nessuno dei due lati è remoto: serve «nome:percorso» per la macchina".into());
    }
    let ui = apri(app);
    // il riquadro remoto mostra la macchina coinvolta
    if let Some(s) = sorg.remoto.clone().or_else(|| dest.remoto.clone()) {
        if ui.remoto.remoto.borrow().as_ref().map(|x| x.nome.clone()) != Some(s.nome.clone()) {
            *ui.remoto.remoto.borrow_mut() = Some(s);
            *ui.remoto.percorso.borrow_mut() = "~".to_string();
            ui.remoto.barra.set_text("~");
            ui.remoto.carica(&ui.tx);
        }
    }
    Ok(accoda_lati(&ui, sorg, dest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tipi_dai_nomi() {
        assert_eq!(tipo_file("cartella1", true), "cartella");
        assert_eq!(tipo_file("zeta.txt", false), "testo");
        assert_eq!(tipo_file("FOTO.PNG", false), "immagine");
        assert_eq!(tipo_file("grande.iso", false), "immagine disco");
        assert_eq!(tipo_file("archivio.tar.gz", false), "archivio");
        assert_eq!(tipo_file("zirkio-term-0.5.0-1-x86_64.pkg.tar.zst", false), "pacchetto");
        assert_eq!(tipo_file("main.rs", false), "Rust");
        assert_eq!(tipo_file("SenzaEstensione", false), "file");
        assert_eq!(tipo_file("mistero.xyz", false), "XYZ");
        assert_eq!(tipo_file(".bashrc", false), "file", "un nome che comincia col punto non è un'estensione");
    }

    fn v(nome: &str, cartella: bool, byte: u64) -> Voce {
        Voce { nome: nome.into(), cartella, byte }
    }

    fn nomi(voci: &[Voce]) -> Vec<&str> {
        voci.iter().map(|v| v.nome.as_str()).collect()
    }

    #[test]
    fn ordina_cartelle_prima_e_per_colonna() {
        let mut voci = vec![
            v("zeta.txt", false, 5),
            v("grande.iso", false, 1_500_000_000),
            v("cartella1", true, 4096),
            v("archivio.zip", false, 2),
            v("..", true, 0),
            v("AltraDir", true, 60),
        ];
        ordina_per(&mut voci, Campo::Nome, true);
        assert_eq!(nomi(&voci), vec!["..", "AltraDir", "cartella1", "archivio.zip", "grande.iso", "zeta.txt"]);

        ordina_per(&mut voci, Campo::Dimensione, true);
        assert_eq!(
            nomi(&voci),
            vec!["..", "AltraDir", "cartella1", "archivio.zip", "zeta.txt", "grande.iso"]
        );

        ordina_per(&mut voci, Campo::Dimensione, false);
        assert_eq!(
            nomi(&voci),
            vec!["..", "cartella1", "AltraDir", "grande.iso", "zeta.txt", "archivio.zip"]
        );

        ordina_per(&mut voci, Campo::Tipo, true);
        // cartelle prima, poi per tipo (IMMAGINE DISCO, TESTO, ZIP → "ARCHIVIO"...)
        assert!(voci[1].cartella && voci[2].cartella);
        assert!(!voci[3].cartella);
    }
}
