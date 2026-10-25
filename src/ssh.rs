//! Sessioni SSH salvate: `~/.config/zirkio-term/ssh.toml`.
//!
//! Un file separato dalla configurazione principale: le sessioni si aggiungono e si
//! tolgono spesso, la configurazione no.  **La password non si salva**: si usa la chiave
//! (eventualmente installata sul remoto la prima volta) oppure la si digita.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Una sessione SSH salvata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sessione {
    pub nome: String,
    #[serde(default)]
    pub descrizione: String,
    pub host: String,
    pub utente: String,
    #[serde(default = "porta_default")]
    pub porta: u16,
    /// Percorso della chiave **privata** (la pubblica si ricava con `.pub`).
    #[serde(default)]
    pub chiave: Option<String>,
    /// Comando da eseguire sul remoto dopo l'accesso.
    #[serde(default)]
    pub comando_remoto: Option<String>,
    /// Alla prossima connessione la chiave pubblica va installata sul remoto
    /// (`ssh-copy-id`, chiede la password una volta).  Poi il segno si toglie.
    #[serde(default)]
    pub invia_chiave: bool,
}

fn porta_default() -> u16 {
    22
}

/// Il comando che spegne la macchina (`@sd <nome>`).
pub const COMANDO_SPEGNI: &str = "sudo shutdown -h now";

impl Default for Sessione {
    fn default() -> Self {
        Sessione {
            nome: String::new(),
            descrizione: String::new(),
            host: String::new(),
            utente: utente_locale(),
            porta: 22,
            chiave: None,
            comando_remoto: None,
            invia_chiave: false,
        }
    }
}

/// Nome dell'utente che sta usando l'app (proposta nel campo «utente»).
pub fn utente_locale() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "root".to_string())
}

impl Sessione {
    /// `utente@host`, come lo vuole `ssh`.
    pub fn destinazione(&self) -> String {
        format!("{}@{}", self.utente.trim(), self.host.trim())
    }

    /// Percorso della chiave **pubblica** (quella che si copia sul remoto).
    pub fn chiave_pubblica(&self) -> Option<String> {
        let k = self.chiave.as_deref()?.trim();
        if k.is_empty() {
            return None;
        }
        if k.ends_with(".pub") {
            Some(k.to_string())
        } else {
            Some(format!("{k}.pub"))
        }
    }

    /// Comando (programma, argomenti) da lanciare nel pannello per connettersi.
    ///
    /// Con `invia_chiave` il primo passaggio installa la chiave pubblica sul remoto e
    /// poi esegue `ssh`: tutto dentro `sh -c`, così la richiesta della password (una
    /// volta sola) si vede nel pannello come in un terminale normale.
    pub fn comando(&self) -> (String, Vec<String>) {
        let remoto = self
            .comando_remoto
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| c.to_string());
        self.comando_con(remoto.as_deref(), false)
    }

    /// Spegne la macchina: `sudo shutdown -h now` con un terminale remoto, perché
    /// senza terminale `sudo` non può chiedere la password (e si fermerebbe).
    pub fn spegni(&self) -> (String, Vec<String>) {
        // Solo per il collaudo: al posto dello spegnimento si può mettere un comando
        // innocuo (`ZIRKIO_SPEGNI_COMANDO`), così la catena si prova senza spegnere
        // nessuna macchina.
        let comando = std::env::var("ZIRKIO_SPEGNI_COMANDO")
            .unwrap_or_else(|_| COMANDO_SPEGNI.to_string());
        self.comando_con(Some(&comando), true)
    }

    /// Come [`Sessione::comando`], ma con il comando remoto scelto da chi chiama.
    pub fn comando_con(&self, remoto: Option<&str>, tty: bool) -> (String, Vec<String>) {
        let dest = self.destinazione();
        let porta = self.porta.to_string();
        let remoto = remoto
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(|c| c.to_string());

        // Chiave effettiva: quella indicata nella sessione, altrimenti la **chiave
        // predefinita** dell'utente (id_ed25519, id_rsa, id_ecdsa).  Prima, con il campo
        // vuoto, il toggle «invia la chiave» non faceva **niente** e la password veniva
        // chiesta lo stesso (segnalato dall'utente il 25/10).
        // Chiave usata: quella **indicata** nella sessione.  Per una connessione
        // normale, se il campo è vuoto non si forza niente (ssh usa le sue predefinite);
        // per l'invio della chiave serve sapere *quale* chiave, quindi si ripiega sulla
        // predefinita dell'utente.
        let (privata, pubblica) = if self.invia_chiave {
            match self.chiave_esistente() {
                Some((p, q)) => (Some(p), Some(q)),
                None => (None, None),
            }
        } else {
            match self.chiave_indicata() {
                Some(k) => {
                    let privata = k.strip_suffix(".pub").unwrap_or(&k).to_string();
                    let pubblica = format!("{privata}.pub");
                    (Some(privata), Some(pubblica))
                }
                None => (None, None),
            }
        };

        let mut ssh: Vec<String> = vec!["-p".into(), porta.clone()];
        if tty {
            // `-t`: terminale remoto.  Serve a `sudo`, che senza terminale non
            // chiede la password e il comando non parte.
            ssh.push("-t".into());
        }
        if let Some(k) = &privata {
            ssh.push("-i".into());
            ssh.push(k.clone());
        }
        ssh.push(dest.clone());
        if let Some(c) = &remoto {
            ssh.push(c.clone());
        }
        let ssh_riga = {
            let mut s = String::from("exec ssh");
            for a in &ssh {
                s.push(' ');
                s.push_str(&q(a));
            }
            s.push('\n');
            s
        };

        if self.invia_chiave {
            let mut script = String::new();
            match (&privata, &pubblica) {
                (Some(privata), Some(pubblica)) => {
                    let ssh_prova = format!(
                        // `-n`: il tentativo non deve leggere l'input del terminale.
                        "ssh -n -o BatchMode=yes -o ConnectTimeout=6 -i {} -p {} {} true",
                        q(privata),
                        porta,
                        q(&dest)
                    );
                    // Nessun messaggio: se la chiave va già bene si entra e basta, altrimenti
                    // ssh-copy-id chiede la password una volta (e stampa lui cosa fa).
                    script.push_str(&format!(
                        "if {} 2>/dev/null; then :; else ssh-copy-id -i {} -p {} {}; fi\n",
                        ssh_prova,
                        q(pubblica),
                        porta,
                        q(&dest)
                    ));
                    script.push_str(&ssh_riga);
                    return ("sh".to_string(), vec!["-c".to_string(), script]);
                }
                _ => {
                    // Chiave non trovata: non si finge niente, si avvisa (senza codici colore,
                    // che in `sh` uscivano alla lettera) e si entra.
                    script.push_str(&format!(
                        "echo \"zirkio-term: nessuna chiave SSH trovata ({}) — entro con la password\"\n",
                        self.chiave_attesa().replace('"', "'")
                    ));
                    script.push_str(&ssh_riga);
                    return ("sh".to_string(), vec!["-c".to_string(), script]);
                }
            }
        }

        let mut args = ssh;
        args.insert(0, String::new());
        (String::from("ssh"), args.into_iter().skip(1).collect())
    }

    /// Percorso della chiave **privata** indicata (o vuoto) e della sua pubblica.
    pub fn chiave_indicata(&self) -> Option<String> {
        let k = self.chiave.as_deref()?.trim();
        if k.is_empty() {
            None
        } else {
            Some(k.to_string())
        }
    }

    /// La chiave da usare **se esiste davvero**: quella indicata, altrimenti la prima
    /// predefinita dell'utente.  Restituisce (privata, pubblica).
    pub fn chiave_esistente(&self) -> Option<(String, String)> {
        if let Some(k) = self.chiave_indicata() {
            let privata = k.strip_suffix(".pub").unwrap_or(&k).to_string();
            let pubblica = format!("{privata}.pub");
            return Some((privata, pubblica));
        }
        chiave_predefinita()
    }

    /// Nome della chiave che si sarebbe voluta usare (per il messaggio d'errore).
    pub fn chiave_attesa(&self) -> String {
        self.chiave_indicata().unwrap_or_else(|| "~/.ssh/id_ed25519".to_string())
    }

    /// Controllo dei campi prima di salvare.  Restituisce il messaggio d'errore.
    pub fn valida(&self) -> Result<(), String> {
        if self.nome.trim().is_empty() {
            return Err("il nome della sessione è obbligatorio".into());
        }
        if self.host.trim().is_empty() {
            return Err("manca l'indirizzo (host) della macchina".into());
        }
        if self.utente.trim().is_empty() {
            return Err("manca l'utente".into());
        }
        if self.porta == 0 {
            return Err("la porta deve stare fra 1 e 65535".into());
        }
        // I valori finiscono in una riga di shell (`sh -c`): niente caratteri che
        // possano chiudere le virgolette o aggiungere comandi.
        for (campo, valore) in [
            ("nome", &self.nome),
            ("host", &self.host),
            ("utente", &self.utente),
        ] {
            if valore.chars().any(|c| c.is_whitespace() || "'\"`$;&|\\".contains(c)) {
                return Err(format!(
                    "il campo «{campo}» non può contenere spazi, virgolette o caratteri di shell"
                ));
            }
        }
        Ok(())
    }

    /// Comando `ssh` (programma, argomenti) per eseguire **un comando** sul remoto, senza
    /// aprire una shell interattiva.  Con `batch` non si ferma a chiedere la password
    /// (serve per sonde e trasferimenti in background).
    pub fn ssh_comando(&self, remoto: &str, batch: bool) -> (String, Vec<String>) {
        let mut args = vec!["-p".to_string(), self.porta.to_string()];
        if let Some((privata, _)) = self.chiave_esistente() {
            args.push("-i".into());
            args.push(privata);
        }
        if batch {
            args.push("-o".into());
            args.push("BatchMode=yes".into());
        }
        args.push("-o".into());
        args.push("ConnectTimeout=10".into());
        args.push(self.destinazione());
        args.push(remoto.to_string());
        ("ssh".to_string(), args)
    }

    /// Opzioni per raggiungere questa macchina con **`scp`** (`-P` per la porta,
    /// `-i` per la chiave): sono le stesse per la sorgente e per la destinazione.
    pub fn opzioni_scp(&self) -> Vec<String> {
        let mut args = vec!["-P".to_string(), self.porta.to_string()];
        if let Some((privata, _)) = self.chiave_esistente() {
            args.push("-i".into());
            args.push(privata);
        }
        args
    }

    /// Nome del file della chiave, per la finestra Informazioni.
    pub fn riassunto(&self) -> String {
        let mut s = format!("{}@{}", self.utente, self.host);
        if self.porta != 22 {
            s.push_str(&format!(" (porta {})", self.porta));
        }
        if let Some(k) = self.chiave.as_deref().filter(|k| !k.trim().is_empty()) {
            s.push_str(&format!(" · chiave {}", k));
        }
        s
    }
}

/// La prima chiave predefinita che esiste in `~/.ssh` (privata e pubblica).
fn chiave_predefinita() -> Option<(String, String)> {
    let home = std::env::var("HOME").ok()?;
    for nome in ["id_ed25519", "id_ecdsa", "id_rsa"] {
        let privata = format!("{home}/.ssh/{nome}");
        let pubblica = format!("{privata}.pub");
        if std::path::Path::new(&privata).exists() && std::path::Path::new(&pubblica).exists() {
            return Some((privata, pubblica));
        }
    }
    None
}

/// Quoting per una shell POSIX: `'…'` con le virgolette singole raddoppiate.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Percorso del file delle sessioni (`ZIRKIO_TERM_SSH` per i collaudi).
pub fn percorso() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ZIRKIO_TERM_SSH") {
        return Some(PathBuf::from(p));
    }
    let dir = crate::config::Config::path()?.parent()?.to_path_buf();
    Some(dir.join("ssh.toml"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileSessioni {
    #[serde(default, rename = "sessione")]
    sessioni: Vec<Sessione>,
}

/// Legge le sessioni salvate.  Un file rotto non blocca l'app: si segnala e si parte
/// senza sessioni (stessa regola della configurazione principale).
pub fn carica() -> Vec<Sessione> {
    let Some(path) = percorso() else { return Vec::new() };
    let Ok(testo) = std::fs::read_to_string(&path) else { return Vec::new() };
    match toml::from_str::<FileSessioni>(&testo) {
        Ok(f) => f.sessioni,
        Err(e) => {
            eprintln!("zirkio-term: {}: {e} — nessuna sessione SSH caricata", path.display());
            Vec::new()
        }
    }
}

/// Scrive le sessioni su disco.
pub fn salva(sessioni: &[Sessione]) -> Result<PathBuf, String> {
    let path = percorso().ok_or("percorso delle sessioni non determinabile")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let f = FileSessioni { sessioni: sessioni.to_vec() };
    let testo = toml::to_string_pretty(&f).map_err(|e| e.to_string())?;
    let intestazione = "# zirkio-term — sessioni SSH (scritte dalla finestra «Sessioni SSH»)\n\
                        # La password NON si salva: si usa la chiave o la si digita.\n\n";
    std::fs::write(&path, format!("{intestazione}{testo}"))
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessione_semplice() -> Sessione {
        Sessione {
            nome: "jan".into(),
            host: "192.0.2.10".into(),
            utente: "utente".into(),
            ..Default::default()
        }
    }

    #[test]
    fn comando_base() {
        let (prog, args) = sessione_semplice().comando();
        assert_eq!(prog, "ssh");
        assert_eq!(args, vec!["-p", "22", "utente@192.0.2.10"]);
    }

    #[test]
    fn comando_con_chiave_e_comando_remoto() {
        let mut s = sessione_semplice();
        s.chiave = Some("/home/utente/.ssh/id_ed25519".into());
        s.comando_remoto = Some("mc".into());
        let (_, args) = s.comando();
        assert_eq!(
            args,
            vec![
                "-p",
                "22",
                "-i",
                "/home/utente/.ssh/id_ed25519",
                "utente@192.0.2.10",
                "mc"
            ]
        );
        assert_eq!(s.chiave_pubblica().as_deref(), Some("/home/utente/.ssh/id_ed25519.pub"));
    }

    #[test]
    fn invio_chiave_usa_ssh_copy_id() {
        let mut s = sessione_semplice();
        s.chiave = Some("/home/utente/.ssh/id_ed25519.pub".into());
        s.invia_chiave = true;
        let (prog, args) = s.comando();
        assert_eq!(prog, "sh");
        assert_eq!(args[0], "-c");
        let script = &args[1];
        println!("---SCRIPT---\n{script}\n---FINE---");
        // prima si prova la chiave senza password: se funziona già non si copia niente
        assert!(script.contains("ssh -n -o BatchMode=yes"), "{script}");
        assert!(!script.contains("chiave già installata"), "niente messaggi: {script}");
        assert!(script.contains("ssh-copy-id -i '/home/utente/.ssh/id_ed25519.pub' -p 22 'utente@192.0.2.10'"), "{script}");
        assert!(script.contains("exec ssh '-p' '22'"), "{script}");
        // con la chiave già .pub non si aggiunge un secondo suffisso
        assert!(!script.contains(".pub.pub"));
    }

    #[test]
    fn invio_chiave_senza_chiave_indicata_avvisa() {
        // senza chiave nel campo si usa quella predefinita; se non esiste si avvisa e si entra
        let mut s = sessione_semplice();
        s.invia_chiave = true;
        s.chiave = None;
        let (prog, args) = s.comando();
        let script = &args[1];
        println!("---SCRIPT SENZA CHIAVE---\n{script}\n---FINE---");
        assert_eq!(prog, "sh");
        assert!(script.contains("exec ssh"), "{script}");
        let predefinita = chiave_predefinita();
        match predefinita {
            Some((privata, _)) => assert!(script.contains(&privata), "{script}"),
            None => assert!(script.contains("nessuna chiave SSH trovata"), "{script}"),
        }
    }

    #[test]
    fn spegnere_usa_un_terminale_remoto() {
        let s = Sessione {
            nome: "201".into(),
            host: "192.0.2.11".into(),
            utente: "utente".into(),
            ..Default::default()
        };
        let (prog, args) = s.spegni();
        assert_eq!(prog, "ssh");
        assert_eq!(args.last().map(String::as_str), Some(COMANDO_SPEGNI));
        assert!(
            args.contains(&"-t".to_string()),
            "senza -t sudo non può chiedere la password: {args:?}"
        );
        assert!(args.contains(&"utente@192.0.2.11".to_string()), "{args:?}");
        // una connessione normale non se lo prende, il terminale remoto
        let (_, senza) = s.comando();
        assert!(!senza.contains(&"-t".to_string()), "{senza:?}");
    }

    #[test]
    fn valida_rifiuta_caratteri_pericolosi() {
        let mut s = sessione_semplice();
        assert!(s.valida().is_ok());
        s.host = "host;rm -rf /".into();
        assert!(s.valida().is_err(), "un host con ; non deve passare");
        s = sessione_semplice();
        s.nome = String::new();
        assert!(s.valida().is_err());
    }

    #[test]
    fn scrittura_e_rilettura() {
        let dir = std::env::temp_dir().join(format!("zirkio-ssh-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ssh.toml");
        unsafe { std::env::set_var("ZIRKIO_TERM_SSH", &path) };
        let mut s = sessione_semplice();
        s.descrizione = "macchina di build".into();
        salva(&[s.clone()]).expect("scrittura");
        let lette = carica();
        assert_eq!(lette.len(), 1);
        assert_eq!(lette[0], s);
        // file rotto -> nessuna sessione, niente panico
        std::fs::write(&path, "[[sessione]\nrotto = ").unwrap();
        assert!(carica().is_empty());
        unsafe { std::env::remove_var("ZIRKIO_TERM_SSH") };
    }
}
