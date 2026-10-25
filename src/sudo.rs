//! Inserimento automatico della password quando `sudo` la chiede.
//!
//! Tre pezzi, tenuti separati apposta:
//!
//! 1. [`Rilevatore`] — guarda il flusso che il programma scrive sul terminale e
//!    riconosce il prompt di `sudo`.  Logica pura, senza GTK e senza system call:
//!    si prova con `cargo test`.
//! 2. I **controlli sul terminale** ([`sudo_davanti`], [`echo_spento`]) — prima di
//!    digitare qualcosa si verifica che davanti al terminale ci sia davvero `sudo`
//!    (e non un `echo "[sudo] password for …"` di passaggio) e che l'eco sia
//!    spento, altrimenti la password finirebbe a video.
//! 3. La **custodia della password** — in RAM (mai su disco) oppure, se l'utente
//!    lo chiede, nel portachiavi di sistema via `secret-tool` (libsecret).
//!
//! L'invio vero e proprio sta in [`invia`], che gira in un thread: aspettare l'eco
//! spento significa dormire, e il ciclo grafico non si blocca.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gtk::prelude::*;

use crate::pty::Maniglia;

/// Processi che chiedono la password di amministrazione.
///
/// `su` è fuori **apposta**: la password che chiede `su` è quella di un altro
/// utente, mandargli quella di sudo sarebbe solo rumore (e un tentativo a vuoto).
const PROC_SUDO: &[&str] = &["sudo", "doas", "pkexec", "sudoedit"];

/// Chiave della macchina locale nel portachiavi.
pub const CHIAVE_LOCALE: &str = "locale";

/// Opzioni di `ssh` che prendono un argomento a parte: senza saltarle, il valore
/// (`BatchMode=yes`, `22`, il file della chiave…) verrebbe scambiato per la
/// macchina di destinazione.
const SSH_OPZIONI_CON_VALORE: &[u8] = b"BbCcDEeFIiJjLlOoPpQRSWw";

/// Parole che compaiono nel prompt di sudo, in giro per il mondo.  Il prompt
/// finisce sempre con `:` — è quello il segnale vero, queste lo confermano.
const MARCATORI: &[&str] = &[
    "[sudo]",
    "password",
    "passwd",
    "mot de passe",
    "passwort",
    "kennwort",
    "contraseña",
    "senha",
    "パスワード",
];

/// Quanto a lungo si considerano "la stessa richiesta" gli invii consecutivi.
/// Oltre, il contatore dei tentativi riparte da zero (un comando nuovo, non un
/// sudo che ha rifiutato la password).
const FINESTRA_TENTATIVI: Duration = Duration::from_secs(25);

/// Quanti invii si fanno per la stessa richiesta: come sudo, tre.
const MAX_TENTATIVI: u8 = 3;

// ------------------------------------------------------------------ rilevatore

/// Riconosce il prompt di `sudo` nel flusso di uscita del programma.
#[derive(Debug, Default)]
pub struct Rilevatore {
    /// Ultimi caratteri **visibili** (senza sequenze di escape), al massimo 400.
    coda: String,
    /// Una sequenza di escape arrivata a metà: si tiene da parte per il pezzo dopo.
    pendente: Vec<u8>,
    /// Quanti invii sono stati fatti per la richiesta in corso.
    invii: u8,
    /// Quando è stato fatto l'ultimo invio.
    istante: Option<Instant>,
}

impl Rilevatore {
    pub fn new() -> Self {
        Rilevatore::default()
    }

    /// Consuma un pezzo di uscita del programma.
    ///
    /// `true` = `sudo` sta chiedendo la password *adesso*: chi chiama deve
    /// mandarla.  Chi chiama non deve contare i tentativi, lo fa il rilevatore.
    pub fn alimenta(&mut self, dati: &[u8]) -> bool {
        let testo = self.pulisci(dati);
        // Solo un a capo: non è una richiesta (e non deve riarmare niente).
        if testo.trim().is_empty() {
            return false;
        }
        self.coda.push_str(&testo);
        tronca(&mut self.coda, 400);

        if !sembra_richiesta(&self.coda) {
            return false;
        }
        let recente = self
            .istante
            .map(|t| t.elapsed() < FINESTRA_TENTATIVI)
            .unwrap_or(false);
        if !recente {
            self.invii = 0;
        }
        if self.invii >= MAX_TENTATIVI {
            // sudo ha già rifiutato: si lascia digitare a mano.
            self.coda.clear();
            return false;
        }
        self.invii += 1;
        self.istante = Some(Instant::now());
        // La coda si svuota: la richiesta è servita.  Così il «nuovo» prompt che
        // sudo stampa dopo un rifiuto si riconosce come richiesta a sé (senza
        // questo, il testo identico del secondo prompt veniva scartato).
        self.coda.clear();
        true
    }

    /// L'utente ha premuto un tasto: se stava per partire un invio, si rinuncia,
    /// e i tentativi contano da capo.
    pub fn nota_tasto(&mut self) {
        self.invii = 0;
        self.istante = None;
        self.coda.clear();
    }

    /// Byte del programma → testo visibile (escape tolti, a capo come spazio).
    fn pulisci(&mut self, dati: &[u8]) -> String {
        let mut grezzo: Vec<u8> = std::mem::take(&mut self.pendente);
        grezzo.extend_from_slice(dati);
        if grezzo.len() > 64 * 1024 {
            grezzo.clear();
            return String::new();
        }
        let mut vis: Vec<u8> = Vec::with_capacity(grezzo.len());
        let mut i = 0;
        while i < grezzo.len() {
            let b = grezzo[i];
            if b == 0x1b {
                // ESC [ … finale nella banda 0x40-0x7e  |  ESC ] … BEL o ESC \
                let Some(&tipo) = grezzo.get(i + 1) else { break };
                match tipo {
                    b'[' => {
                        let mut j = i + 2;
                        while j < grezzo.len() && !(0x40..=0x7e).contains(&grezzo[j]) {
                            j += 1;
                        }
                        if j >= grezzo.len() {
                            break; // sequenza incompleta: si tiene per il pezzo dopo
                        }
                        i = j + 1;
                    }
                    b']' => {
                        let mut j = i + 2;
                        while j < grezzo.len()
                            && grezzo[j] != 0x07
                            && !(grezzo[j] == 0x1b && grezzo.get(j + 1) == Some(&b'\\'))
                        {
                            j += 1;
                        }
                        if j >= grezzo.len() {
                            break;
                        }
                        i = if grezzo[j] == 0x07 { j + 1 } else { j + 2 };
                    }
                    _ => i += 2,
                }
            } else {
                match b {
                    b'\r' => {}
                    b'\n' => vis.push(b' '),
                    0x00..=0x1f | 0x7f => {}
                    // Sotto 0x20 già scartati; qui resta l'ASCII stampabile e il
                    // UTF-8 (che si ricompone alla fine con `from_utf8_lossy`).
                    _ => vis.push(b),
                }
                i += 1;
            }
        }
        if i < grezzo.len() {
            let resto = grezzo[i..].to_vec();
            // Una sequenza lunghissima non è una sequenza: si butta.
            if resto.len() <= 64 {
                self.pendente = resto;
            }
        }
        String::from_utf8_lossy(&vis).to_string()
    }
}

/// Il testo finisce con `:` e contiene una parola da prompt di password?
fn sembra_richiesta(coda: &str) -> bool {
    let t = coda.trim_end();
    if !t.ends_with(':') {
        return false;
    }
    let basso = t.to_ascii_lowercase();
    let coda_bassa = ultimi(&basso, 160);
    MARCATORI.iter().any(|m| coda_bassa.contains(m))
}

/// Ultimi `n` **caratteri** (non byte: il taglio deve cadere su un confine).
fn ultimi(s: &str, n: usize) -> &str {
    match s.char_indices().rev().nth(n) {
        Some((i, _)) => &s[i..],
        None => s,
    }
}

/// Tiene solo gli ultimi `n` caratteri.
fn tronca(s: &mut String, n: usize) {
    let taglia = s.len() - ultimi(s, n).len();
    if taglia > 0 {
        s.drain(..taglia);
    }
}

// ------------------------------------------------------- controlli sul terminale

/// Chi chiede la password: la chiave dice **quale** password usare.
///
/// Ogni macchina ha la sua password di sudo, quindi se ne tiene una per macchina:
/// `locale` per questo computer, la destinazione SSH per le sessioni remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bersaglio {
    /// Chiave stabile nel portachiavi: `locale` oppure `utente@macchina`.
    pub chiave: String,
    /// Come si chiama nella finestrella («questo computer (giovanni-pc)»).
    pub nome: String,
}

impl Bersaglio {
    pub fn locale() -> Bersaglio {
        Bersaglio {
            chiave: CHIAVE_LOCALE.to_string(),
            nome: format!("questo computer ({})", nome_host()),
        }
    }
}

/// Chi sta chiedendo la password, se qualcuno la sta chiedendo **davvero**.
///
/// Serve a non credere a chiunque **scriva** il prompt: `echo "[sudo] password
/// for utente:"` non deve far uscire la password.  Si guarda il gruppo di
/// processi in primo piano della pty (`/proc/<pid>/comm`):
///
/// - `sudo`/`doas`/`pkexec` → la password di **questo** computer;
/// - `ssh` → la password di **quella** macchina, che si legge dalla sua riga di
///   comando (`/proc/<pid>/cmdline`): dentro una sessione remota il `sudo` che
///   chiede è lontano, e davanti al nostro terminale c'è `ssh`.
pub fn bersaglio(m: &Maniglia) -> Option<Bersaglio> {
    let pgrp = unsafe { libc::tcgetpgrp(m.fd()) };
    if pgrp <= 0 {
        return None;
    }
    let comm = std::fs::read_to_string(format!("/proc/{pgrp}/comm")).ok()?;
    let comm = comm.trim().to_string();
    if comm.is_empty() {
        return None;
    }
    if nomi_accettati().iter().any(|n| *n == comm) {
        return Some(Bersaglio::locale());
    }
    if comm == "ssh" {
        let riga = std::fs::read(format!("/proc/{pgrp}/cmdline")).ok()?;
        let args: Vec<String> = riga
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8_lossy(a).to_string())
            .collect();
        let macchina = destinazione_ssh(&args)?;
        return Some(Bersaglio {
            nome: macchina.clone(),
            chiave: macchina,
        });
    }
    None
}

/// La macchina di destinazione di una riga di comando `ssh`.
///
/// `["ssh", "-p", "22", "utente@192.0.2.11"]` → `utente@192.0.2.11`.
/// Funzione pura, provata con `cargo test`.
pub fn destinazione_ssh(args: &[String]) -> Option<String> {
    let mut i = 1; // args[0] è il programma
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--" {
            return args.get(i + 1).and_then(|s| normalizza_macchina(s));
        }
        if let Some(corpo) = a.strip_prefix('-') {
            let lettera = corpo.chars().next()?;
            // `-p 22` (valore a parte) occupa due argomenti, `-p22` uno solo.
            let attaccato = corpo.chars().count() > 1;
            if SSH_OPZIONI_CON_VALORE.contains(&(lettera as u8)) && !attaccato {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        return normalizza_macchina(a);
    }
    None
}

/// Toglie la porta (`macchina:22`) e le parentesi dell'IPv6 (`[::1]:22`).
fn normalizza_macchina(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(resto) = s.strip_prefix('[') {
        let (host, _) = resto.split_once(']')?;
        return if host.is_empty() {
            None
        } else {
            Some(host.to_string())
        };
    }
    if let Some((host, porta)) = s.rsplit_once(':') {
        if !porta.is_empty() && porta.chars().all(|c| c.is_ascii_digit()) {
            return Some(host.to_string());
        }
    }
    Some(s.to_string())
}

fn nome_host() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "questo computer".to_string())
}

/// Nomi di processo che valgono come «sudo».  `ZIRKIO_SUDO_PROC` esiste solo per
/// il collaudo: la finta sudo dei test è uno script, e il suo `comm` è quello
/// dell'interprete (`sh`).
fn nomi_accettati() -> Vec<String> {
    let mut v: Vec<String> = PROC_SUDO.iter().map(|s| s.to_string()).collect();
    if let Ok(extra) = std::env::var("ZIRKIO_SUDO_PROC") {
        v.extend(
            extra
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
        );
    }
    v
}

/// L'eco è spento sul terminale?  `sudo` lo spegne subito prima di leggere: se
/// scrivessimo un istante prima, la password comparirebbe sullo schermo.
pub fn echo_spento(m: &Maniglia) -> bool {
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(m.fd(), &mut t) } != 0 {
        return false;
    }
    (t.c_lflag & libc::ECHO as libc::tcflag_t) == 0
}

/// Aspetta (al massimo `ms`) che l'eco si spenga.  `true` se è spento.
fn attendi_echo_spento(m: &Maniglia, ms: u64) -> bool {
    let scadenza = Instant::now() + Duration::from_millis(ms);
    loop {
        if echo_spento(m) {
            return true;
        }
        if Instant::now() >= scadenza {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Scrive la password nella sessione e preme Invio.
///
/// Blocca (aspetta l'eco spento): va chiamata da un thread, mai dal ciclo grafico.
/// `false` se non si è scritto niente — l'utente ha premuto un tasto, non c'è più
/// sudo davanti, o l'eco non si è spento in tempo.
pub fn invia(m: Maniglia, mut pw: String, annulla: &AtomicBool, chiave: &str) -> bool {
    if annulla.load(Ordering::Relaxed) {
        return false;
    }
    // Un attimo di respiro prima di scrivere.  Con `sudo` in locale l'eco spento è
    // la prova che sta leggendo; dentro una sessione SSH invece l'eco locale è
    // **già** spento da `ssh` (mette il terminale in raw mode), quindi non dice
    // niente sull'eco del terminale remoto: questa pausa lascia a `sudo` il tempo
    // di spegnerlo anche là, altrimenti la password comparirebbe a schermo.
    std::thread::sleep(Duration::from_millis(120));
    if !attendi_echo_spento(&m, 1500) {
        azzera(&mut pw);
        return false;
    }
    // Chi sta leggendo deve essere ancora lo stesso: se nel frattempo `sudo` è
    // finito e davanti c'è altro, la password non si scrive.
    let stesso = matches!(bersaglio(&m), Some(b) if b.chiave == chiave);
    if annulla.load(Ordering::Relaxed) || !stesso {
        azzera(&mut pw);
        return false;
    }
    pw.push('\n');
    m.scrivi(pw.as_bytes());
    azzera(&mut pw);
    if std::env::var_os("ZIRKIO_STATS").is_some() {
        eprintln!("[stats] sudo: password inviata");
    }
    true
}

/// Sovrascrive il contenuto della stringa.
///
/// È una riduzione della finestra, non una garanzia: la password può essere già
/// finita in una copia (o in swap).  Serve a non lasciarla in giro nella RAM del
/// processo quando non serve più.
fn azzera(s: &mut String) {
    unsafe { std::ptr::write_bytes(s.as_mut_ptr(), 0, s.len()) };
    s.clear();
}

// -------------------------------------------------------------- la password

/// Una password per macchina: `(chiave, password)`.
static PW: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// La password in memoria per quella macchina (nessun disco, nessun portachiavi).
pub fn password(chiave: &str) -> Option<String> {
    if std::env::var("ZIRKIO_SUDO_TEST").as_deref() == Ok("1") {
        // Solo per il collaudo: la password si passa dall'ambiente, perché il test
        // non ha un portachiavi e non deve toccare quello vero.
        if let Ok(p) = std::env::var("ZIRKIO_SUDO_PASSWORD") {
            if !p.is_empty() && !c_e_password(chiave) {
                imposta(chiave, &p);
            }
        }
    }
    let g = PW.lock().ok()?;
    g.iter().find(|(k, _)| k == chiave).map(|(_, v)| v.clone())
}

pub fn c_e_password(chiave: &str) -> bool {
    PW.lock()
        .map(|g| g.iter().any(|(k, _)| k == chiave))
        .unwrap_or(false)
}

/// Le macchine per cui c'è una password in memoria.
pub fn macchine_note() -> Vec<String> {
    PW.lock()
        .map(|g| g.iter().map(|(k, _)| k.clone()).collect())
        .unwrap_or_default()
}

pub fn imposta(chiave: &str, pw: &str) {
    if let Ok(mut g) = PW.lock() {
        if let Some(i) = g.iter().position(|(k, _)| k == chiave) {
            azzera(&mut g[i].1);
            g[i].1 = pw.to_string();
        } else {
            g.push((chiave.to_string(), pw.to_string()));
        }
    }
}

pub fn dimentica(chiave: &str) {
    if let Ok(mut g) = PW.lock() {
        if let Some(i) = g.iter().position(|(k, _)| k == chiave) {
            azzera(&mut g[i].1);
            g.remove(i);
        }
    }
}

// -------------------------------------------------------------- portachiavi

const ETICHETTA: &str = "Zirkio Terminal — password di sudo";

fn secret_tool() -> Option<std::path::PathBuf> {
    let perc = std::env::var_os("PATH")?;
    std::env::split_paths(&perc)
        .map(|d| d.join("secret-tool"))
        .find(|p| p.is_file())
}

/// C'è `secret-tool` (libsecret) su questa macchina?
pub fn portachiavi_disponibile() -> bool {
    secret_tool().is_some()
}

/// Legge dal portachiavi la password di **quella** macchina.
pub fn dal_portachiavi(chiave: &str) -> Option<String> {
    let out = comando(
        &["lookup", "service", "zirkio-term", "account", "sudo", "macchina", chiave],
        None,
    )?;
    let p = out.trim_end_matches(['\n', '\r']).to_string();
    if p.is_empty() { None } else { Some(p) }
}

/// Quanto si aspetta `secret-tool` prima di lasciar perdere.
///
/// ⚠️ Senza un servizio di portachiavi (`gnome-keyring`/`KWallet` dietro D-Bus)
/// `secret-tool` **non esce**: resta lì ad aspettare per sempre.  Visto succedere
/// sul PC di collaudo, dove il binario c'è ma il servizio no.
const TIMEOUT_PORTACHIAVI: Duration = Duration::from_millis(2000);

/// Scrive la password nel portachiavi di sistema.  La password passa da stdin,
/// **non** dalla riga di comando: `ps` la vedrebbe.
pub fn nel_portachiavi(chiave: &str, pw: &str) -> Result<(), String> {
    comando(
        &[
            "store",
            "--label",
            ETICHETTA,
            "service",
            "zirkio-term",
            "account",
            "sudo",
            "macchina",
            chiave,
        ],
        Some(pw),
    )
    .map(|_| ())
    .ok_or_else(|| {
        if portachiavi_disponibile() {
            "il portachiavi non ha risposto: è sbloccato? c'è un servizio di portachiavi \
             (gnome-keyring o KWallet) in questa sessione?"
                .to_string()
        } else {
            "secret-tool non è installato (pacchetto libsecret)".to_string()
        }
    })
}

pub fn cancella_portachiavi(chiave: &str) -> Result<(), String> {
    comando(
        &["clear", "service", "zirkio-term", "account", "sudo", "macchina", chiave],
        None,
    )
    .map(|_| ())
    .ok_or_else(|| "non ho potuto togliere la password dal portachiavi".to_string())
}

fn comando(args: &[&str], stdin: Option<&str>) -> Option<String> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    let prog = secret_tool()?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut figlio = cmd.spawn().ok()?;
    if let Some(segreto) = stdin {
        let mut g = figlio.stdin.take()?;
        let _ = g.write_all(segreto.as_bytes());
        let _ = g.write_all(b"\n");
        // `g` cade qui: il figlio vede la fine dell'input.  Senza, `store` aspetta.
    }
    let scadenza = Instant::now() + TIMEOUT_PORTACHIAVI;
    loop {
        match figlio.try_wait() {
            Ok(Some(stato)) => {
                let mut out = String::new();
                if let Some(mut o) = figlio.stdout.take() {
                    let _ = o.read_to_string(&mut out);
                }
                return if stato.success() { Some(out) } else { None };
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        if Instant::now() >= scadenza {
            let _ = figlio.kill();
            let _ = figlio.wait();
            if std::env::var_os("ZIRKIO_STATS").is_some() {
                eprintln!("[stats] sudo: il portachiavi non risponde, lascio perdere");
            }
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ------------------------------------------------------------------- finestre

#[allow(clippy::too_many_arguments)]
fn finestra_password(
    titolo: &str,
    padre: &impl IsA<gtk::Window>,
    spiegazione: &str,
    con_cancella: bool,
    ricorda_iniziale: bool,
) -> (gtk::Dialog, gtk::Entry, gtk::CheckButton) {
    let pulsanti: Vec<(&str, gtk::ResponseType)> = if con_cancella {
        vec![
            ("Cancella", gtk::ResponseType::Other(1)),
            ("Annulla", gtk::ResponseType::Cancel),
            ("Salva", gtk::ResponseType::Accept),
        ]
    } else {
        vec![
            ("Annulla", gtk::ResponseType::Cancel),
            ("Invia", gtk::ResponseType::Accept),
        ]
    };
    let d = gtk::Dialog::with_buttons(
        Some(titolo),
        Some(padre),
        gtk::DialogFlags::MODAL,
        &pulsanti,
    );
    d.set_default_response(gtk::ResponseType::Accept);
    d.set_resizable(false);
    let area = d.content_area();
    area.set_spacing(10);
    area.set_border_width(16);
    let testo = gtk::Label::new(Some(spiegazione));
    testo.set_xalign(0.0);
    testo.set_line_wrap(true);
    testo.set_max_width_chars(52);
    let campo = gtk::Entry::new();
    campo.set_visibility(false);
    campo.set_placeholder_text(Some("password di sudo"));
    campo.set_activates_default(true);
    campo.set_input_purpose(gtk::InputPurpose::Password);
    // ⚠️ Qui NON si interroga il portachiavi: questo codice gira nel ciclo grafico
    // e `secret-tool` può bloccarsi (senza servizio non esce più).  Lo stato
    // iniziale arriva dalle preferenze, che l'utente ha già scelto.
    let ricorda = gtk::CheckButton::with_label("Ricorda nel portachiavi di sistema");
    ricorda.set_active(ricorda_iniziale && portachiavi_disponibile());
    ricorda.set_sensitive(portachiavi_disponibile());
    if !portachiavi_disponibile() {
        ricorda.set_tooltip_text(Some(
            "secret-tool non è installato: la password resta solo in memoria, per questa sessione",
        ));
    }
    area.pack_start(&testo, false, false, 0);
    area.pack_start(&campo, false, false, 0);
    area.pack_start(&ricorda, false, false, 0);
    (d, campo, ricorda)
}

fn avviso(padre: &impl IsA<gtk::Window>, msg: &str) {
    let d = gtk::MessageDialog::new(
        Some(padre),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Warning,
        gtk::ButtonsType::Close,
        msg,
    );
    d.connect_response(|d, _| d.close());
    d.show_all();
}

/// Chiede la password perché sudo l'ha appena chiesta e non ne abbiamo una.
///
/// Accettando, la password si manda subito (`invia`, in un thread) e resta in
/// memoria; se la casella è spuntata va anche nel portachiavi.
pub fn dialogo_password(
    padre: &gtk::ApplicationWindow,
    maniglia: Maniglia,
    annulla: Arc<AtomicBool>,
    bersaglio: Bersaglio,
    ricorda_iniziale: bool,
) {
    // La macchina si dice nel titolo e nel testo: la password di ognuna è diversa.
    let titolo = format!("Password di sudo per {} — Zirkio Terminal", bersaglio.nome);
    let testo = format!(
        "sudo su {} sta chiedendo la password e non ce n'è una in memoria.\n\
         Resta solo in memoria, per questa sessione, a meno che tu non scelga il portachiavi.\n\
         La password di ogni macchina si tiene separata.",
        bersaglio.nome
    );
    let (d, campo, ricorda) = finestra_password(&titolo, padre, &testo, false, ricorda_iniziale);
    let d2 = d.clone();
    let campo2 = campo.clone();
    let ricorda2 = ricorda.clone();
    let padre2 = padre.clone();
    let chiave = bersaglio.chiave.clone();
    d.connect_response(move |_, risposta| {
        if risposta == gtk::ResponseType::Accept {
            let pw = campo2.text().to_string();
            if !pw.is_empty() {
                imposta(&chiave, &pw);
                if ricorda2.is_active() {
                    if let Err(e) = nel_portachiavi(&chiave, &pw) {
                        avviso(&padre2, &e);
                    }
                }
                let m = maniglia.clone();
                let a = annulla.clone();
                let ch = chiave.clone();
                std::thread::spawn(move || {
                    invia(m, pw, &a, &ch);
                });
            }
        }
        d2.close();
    });
    d.show_all();
    campo.grab_focus();
}

/// Finestra «password di sudo» delle Preferenze: salva, cancella, dimentica.
/// `al_cambio` aggiorna l'etichetta di stato nella finestra che l'ha aperta.
pub fn dialogo_imposta(
    padre: &impl IsA<gtk::Window>,
    al_cambio: std::rc::Rc<dyn Fn()>,
    ricorda_iniziale: bool,
) {
    let (d, campo, ricorda) = finestra_password(
        &format!("Password di sudo per {} — Zirkio Terminal", Bersaglio::locale().nome),
        padre,
        "La password di questo computer: si usa quando sudo la chiede qui.\n\
         Quelle delle altre macchine si chiedono la prima volta che servono, nella sessione \
         in cui servono, e restano separate.\n\
         In memoria vale fino alla chiusura dell'app; con il portachiavi anche dopo.",
        true,
        ricorda_iniziale,
    );
    let d2 = d.clone();
    let campo2 = campo.clone();
    let ricorda2 = ricorda.clone();
    let padre2 = padre.clone();
    let al_cambio2 = al_cambio.clone();
    d.connect_response(move |_, risposta| {
        match risposta {
            gtk::ResponseType::Accept => {
                let pw = campo2.text().to_string();
                if !pw.is_empty() {
                    imposta(CHIAVE_LOCALE, &pw);
                    if ricorda2.is_active() {
                        if let Err(e) = nel_portachiavi(CHIAVE_LOCALE, &pw) {
                            avviso(&padre2, &e);
                        }
                    } else {
                        // togliere il segno vuol dire togliere anche dal portachiavi
                        let _ = cancella_portachiavi(CHIAVE_LOCALE);
                    }
                    al_cambio2();
                }
            }
            gtk::ResponseType::Other(1) => {
                dimentica(CHIAVE_LOCALE);
                // «Cancella» vuole togliere anche dal portachiavi; se non risponde
                // non è grave: la password in memoria è già andata.
                if portachiavi_disponibile() && cancella_portachiavi(CHIAVE_LOCALE).is_err() {
                    eprintln!("[sudo] portachiavi non raggiungibile: password tolta solo dalla memoria");
                }
                al_cambio2();
            }
            _ => {}
        }
        d2.close();
    });
    d.show_all();
    campo.grab_focus();
}

// --------------------------------------------------------------------- prove

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn riconosce_il_prompt_normale() {
        let mut r = Rilevatore::new();
        assert!(r.alimenta(b"[sudo] password for utente: "));
    }

    #[test]
    fn riconosce_il_prompt_in_italiano_e_altre_lingue() {
        for p in [
            "[sudo] password di utente: ",
            "Password: ",
            "[sudo] Mot de passe de utente : ",
            "[sudo] Passwort für utente: ",
        ] {
            let mut r = Rilevatore::new();
            assert!(r.alimenta(p.as_bytes()), "non riconosciuto: {p}");
        }
    }

    #[test]
    fn prompt_spezzato_in_due_letture() {
        let mut r = Rilevatore::new();
        assert!(!r.alimenta(b"[sudo] pass"));
        assert!(r.alimenta(b"word for utente: "));
    }

    #[test]
    fn prompt_colorato() {
        let mut r = Rilevatore::new();
        assert!(r.alimenta(b"\x1b[1;31m[sudo]\x1b[0m password for utente: "));
    }

    #[test]
    fn escape_spezzata_fra_due_letture() {
        let mut r = Rilevatore::new();
        assert!(!r.alimenta(b"\x1b["));
        // l'escape si ricompone: il testo non deve contenere spazzatura
        assert!(r.alimenta(b"32m[sudo] password for utente: "));
    }

    #[test]
    fn una_riga_vuota_non_fa_scattare_nulla() {
        let mut r = Rilevatore::new();
        assert!(!r.alimenta(b"ciao\n"));
        assert!(!r.alimenta(b"\n"));
        assert!(!r.alimenta(b"utente@pc:~$ ls\n"));
    }

    #[test]
    fn il_capo_dopo_la_password_non_raddoppia_l_invio() {
        let mut r = Rilevatore::new();
        assert!(r.alimenta(b"[sudo] password for utente: "));
        assert!(!r.alimenta(b"\n"), "l'a capo non è una seconda richiesta");
        assert!(!r.alimenta(b"\r\n"));
    }

    #[test]
    fn dopo_un_rifiuto_riprova_e_poi_si_ferma() {
        let mut r = Rilevatore::new();
        assert!(r.alimenta(b"[sudo] password for utente: "), "1º");
        assert!(
            r.alimenta(b"\nSorry, try again.\n[sudo] password for utente: "),
            "2º tentativo"
        );
        assert!(
            r.alimenta(b"\nSorry, try again.\n[sudo] password for utente: "),
            "3º tentativo"
        );
        assert!(
            !r.alimenta(b"\nSorry, try again.\n[sudo] password for utente: "),
            "al quarto si smette e si lascia digitare"
        );
    }

    #[test]
    fn un_tasto_dell_utente_azzera_i_tentativi() {
        let mut r = Rilevatore::new();
        assert!(r.alimenta(b"[sudo] password for utente: "));
        r.nota_tasto();
        assert!(r.alimenta(b"[sudo] password for utente: "));
    }

    #[test]
    fn il_solo_testo_scatta_e_lo_ferma_solo_proc() {
        // Un finto prompt è indistinguibile **nel testo**: chi lo scrive a mano
        // (`echo "[sudo] password for utente: "`) fa scattare il rilevatore.  È il
        // controllo su `/proc` (`sudo_davanti`) a fermare l'invio; qui si fissa
        // che il testo, da solo, scatta.
        let mut r = Rilevatore::new();
        assert!(r.alimenta(b"[sudo] password for utente: "));
    }

    /// `secret-tool` senza servizio di portachiavi **non esce**: il comando deve
    /// accorgersene e lasciar perdere, altrimenti la finestra delle preferenze
    /// resta appesa (visto sul PC di collaudo, dove il binario c'è e il servizio no).
    #[test]
    fn un_portachiavi_che_non_risponde_non_blocca() {
        static ENV2: Mutex<()> = Mutex::new(());
        let _guard = ENV2.lock().unwrap_or_else(|e| e.into_inner());
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("zirkio-secret-finto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let finto = dir.join("secret-tool");
        std::fs::write(&finto, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&finto, std::fs::Permissions::from_mode(0o755)).unwrap();
        // In testa al PATH: così gli altri comandi dei test restano raggiungibili.
        let vecchio = std::env::var_os("PATH").unwrap_or_default();
        let nuovo = format!("{}:{}", dir.display(), vecchio.to_string_lossy());
        unsafe { std::env::set_var("PATH", &nuovo) };
        let inizio = Instant::now();
        assert_eq!(
            dal_portachiavi(CHIAVE_LOCALE),
            None,
            "un portachiavi muto non tiene una password"
        );
        let durata = inizio.elapsed();
        assert!(
            durata < Duration::from_secs(6),
            "il comando si è bloccato per {durata:?}"
        );
        unsafe { std::env::set_var("PATH", &vecchio) };
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// End-to-end senza display: pty vera, finta sudo, rilevatore e invio veri.
    ///
    /// La finta sudo è uno script `sh`: il suo `comm` è `sh`, quindi si usa
    /// `ZIRKIO_SUDO_PROC` per farla accettare come processo di amministrazione.
    #[test]
    fn la_password_arriva_a_una_finta_sudo() {
        static ENV: Mutex<()> = Mutex::new(());
        let _guard = ENV.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { std::env::set_var("ZIRKIO_SUDO_PROC", "sh") };

        let raccolto: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let per_sink = raccolto.clone();
        let sink = crate::pty::Sink {
            on_data: Box::new(move |d| {
                per_sink.lock().unwrap().extend_from_slice(&d);
            }),
            on_exit: Box::new(|| {}),
        };
        let mut pty = crate::pty::Pty::spawn(
            "/bin/sh",
            &[
                "-c".to_string(),
                "printf '[sudo] password for utente: '; stty -echo; \
                 IFS= read -r pw; stty echo; printf '\\npreso:%s\\n' \"$pw\""
                    .to_string(),
            ],
            80,
            24,
            None,
            sink,
        )
        .expect("pty");

        let preleva = |r: &Arc<Mutex<Vec<u8>>>| {
            let mut g = r.lock().unwrap();
            std::mem::take(&mut *g)
        };

        let mut det = Rilevatore::new();
        let mut visto = false;
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(10));
            let nuovi = preleva(&raccolto);
            if !nuovi.is_empty() && det.alimenta(&nuovi) {
                visto = true;
                break;
            }
        }
        assert!(visto, "la finta sudo non è stata riconosciuta");

        let maniglia = pty.maniglia().expect("maniglia della pty");
        let chi = bersaglio(&maniglia).expect("il processo in primo piano deve chiedere la password");
        assert_eq!(chi.chiave, CHIAVE_LOCALE, "la finta sudo è locale");
        let annulla = AtomicBool::new(false);
        assert!(
            invia(maniglia, "segreta123".to_string(), &annulla, CHIAVE_LOCALE),
            "la password non è partita (eco non spento?)"
        );

        let mut testo = String::new();
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(10));
            testo.push_str(&String::from_utf8_lossy(&preleva(&raccolto)));
            if testo.contains("preso:") {
                break;
            }
        }
        assert!(
            testo.contains("preso:segreta123"),
            "la finta sudo ha letto: {testo:?}"
        );
        pty.termina();
        unsafe { std::env::remove_var("ZIRKIO_SUDO_PROC") };
    }

    #[test]
    fn riconosce_la_macchina_di_una_sessione_ssh() {
        let casi: Vec<(Vec<&str>, Option<&str>)> = vec![
            (vec!["ssh", "utente@192.0.2.11"], Some("utente@192.0.2.11")),
            (
                vec!["ssh", "-p", "22", "utente@192.0.2.11"],
                Some("utente@192.0.2.11"),
            ),
            (
                vec![
                    "ssh", "-o", "BatchMode=yes", "-i", "/home/utente/.ssh/id_ed25519", "utente@host",
                ],
                Some("utente@host"),
            ),
            (vec!["ssh", "-p22", "host.example"], Some("host.example")),
            (vec!["ssh", "-oBatchMode=yes", "host"], Some("host")),
            (vec!["ssh", "--", "host"], Some("host")),
            (vec!["ssh", "host.example:2222"], Some("host.example")),
            (vec!["ssh", "[::1]:22"], Some("::1")),
            (vec!["ssh"], None),
            (vec!["ssh", "-v"], None),
        ];
        for (riga, atteso) in casi {
            let args: Vec<String> = riga.iter().map(|s| s.to_string()).collect();
            assert_eq!(
                destinazione_ssh(&args).as_deref(),
                atteso,
                "riga di comando non capita: {riga:?}"
            );
        }
    }

    #[test]
    fn ogni_macchina_ha_la_sua_password() {
        imposta(CHIAVE_LOCALE, "uno");
        imposta("utente@192.0.2.11", "due");
        assert_eq!(password(CHIAVE_LOCALE).as_deref(), Some("uno"));
        assert_eq!(password("utente@192.0.2.11").as_deref(), Some("due"));
        assert_eq!(password("mai-vista"), None, "una macchina nuova non ha password");
        dimentica(CHIAVE_LOCALE);
        assert!(!c_e_password(CHIAVE_LOCALE));
        assert!(
            c_e_password("utente@192.0.2.11"),
            "dimenticarne una non deve toccare le altre"
        );
        dimentica("utente@192.0.2.11");
        let _ = &super::macchine_note;
    }

    #[test]
    fn il_prompt_di_ssh_non_finisce_con_i_due_punti_giusti() {
        let mut r = Rilevatore::new();
        assert!(!r.alimenta(b"Are you sure you want to continue connecting (yes/no/[fingerprint])? "));
    }
}
