//! Trasferimento di file e cartelle (`scp`) fra questo computer e le macchine
//! delle sessioni SSH salvate.
//!
//! **Una sola sintassi per tutte le direzioni**: `<sorgente> <destinazione>`, dove un
//! lato è remoto se ha il prefisso `<sessione>:`
//!
//! ```text
//! @scp ~/foto.jpg 207:~/Immagini/     invio di un file
//! @scp ~/progetto 207:/tmp/           invio di una cartella
//! @scp 207:~/Immagini/foto.jpg ~/     ricezione
//! @scp 207:/var/log 201:~/logs/       da una macchina all'altra (`scp -3`)
//! ```
//!
//! Non si dice se è un file o una cartella: lo scopre `rileva` (`stat` in locale,
//! `test -d` / `stat` sul remoto) e da lì nasce l'opzione `-r`.  Le funzioni che
//! parlano con la rete (`rileva`) sono **bloccanti**: vanno chiamate da un thread,
//! mai dal thread grafico.
//!
//! Questo modulo non conosce GTK: la finestra e la coda stanno in `scpwin.rs`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::ssh::Sessione;

/// Un lato del trasferimento: questo computer (`remoto = None`) oppure una macchina.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lato {
    pub remoto: Option<Sessione>,
    /// Percorso come lo vede l'utente (`~` resta `~` sul remoto).
    pub percorso: String,
}

/// Che cosa c'è nel percorso: lo decide `rileva`, non l'utente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    File,
    Cartella,
}

impl Tipo {
    pub fn nome(self) -> &'static str {
        match self {
            Tipo::File => "file",
            Tipo::Cartella => "cartella",
        }
    }
}

/// Quanto è grande e di che tipo è: serve alla finestra per dirlo *prima* di partire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Info {
    pub tipo: Tipo,
    pub byte: u64,
    /// Numero di file dentro la cartella (1 per un file).
    pub elementi: u64,
}

impl Info {
    /// «cartella · 43 file · 1,2 GB» oppure «file · 12,4 MB».
    pub fn descrizione(&self) -> String {
        match self.tipo {
            Tipo::File => format!("{} · {}", self.tipo.nome(), formatta_byte(self.byte)),
            Tipo::Cartella => format!(
                "{} · {} file · {}",
                self.tipo.nome(),
                self.elementi,
                formatta_byte(self.byte)
            ),
        }
    }
}

impl Lato {
    /// Lato locale con il percorso espanso (`~`) e reso **assoluto** rispetto a `cwd`.
    pub fn locale(percorso: &str, cwd: Option<&Path>) -> Lato {
        Lato {
            remoto: None,
            percorso: espandi(percorso, cwd),
        }
    }

    /// Come si scrive questo lato in una riga (`207:~/Documenti`).
    pub fn etichetta(&self) -> String {
        match &self.remoto {
            Some(s) => format!("{}:{}", s.nome, self.percorso),
            None => self.percorso.clone(),
        }
    }

    /// Il lato come lo vuole `scp`: percorso locale, oppure `utente@host:percorso`.
    pub fn spec_scp(&self) -> String {
        match &self.remoto {
            Some(s) => format!("{}:{}", s.destinazione(), self.percorso),
            None => self.percorso.clone(),
        }
    }
}

/// Legge un lato: `<sessione>:<percorso>`, `<utente>@<host>:<percorso>` o un percorso locale.
///
/// I due punti separano **solo** se prima non c'è una barra: così `./strano:nome` resta
/// locale.  `207:` (senza percorso) vale la cartella personale.
pub fn analizza(
    spec: &str,
    sessioni: &[Sessione],
    cwd: Option<&Path>,
) -> Result<Lato, String> {
    let spec = spec.trim();
    if spec.is_empty() {
        return Err("percorso vuoto".into());
    }
    if let Some((testa, coda)) = spec.split_once(':') {
        if !testa.is_empty() && !testa.contains('/') {
            let sessione = cerca_sessione(testa, sessioni)?;
            let percorso = if coda.trim().is_empty() {
                "~".to_string()
            } else {
                coda.trim().to_string()
            };
            return Ok(Lato { remoto: Some(sessione), percorso });
        }
    }
    Ok(Lato::locale(spec, cwd))
}

/// Nome di sessione salvata, oppure `utente@host` (o solo `host`).
fn cerca_sessione(testa: &str, sessioni: &[Sessione]) -> Result<Sessione, String> {
    if let Some(s) = sessioni.iter().find(|s| s.nome == testa) {
        return Ok(s.clone());
    }
    if testa.contains('@') || testa.contains('.') || testa.parse::<std::net::IpAddr>().is_ok() {
        let (utente, host) = match testa.split_once('@') {
            Some((u, h)) => (u.to_string(), h.to_string()),
            None => (crate::ssh::utente_locale(), testa.to_string()),
        };
        if host.is_empty() || utente.is_empty() {
            return Err(format!("«{testa}» non è una macchina valida"));
        }
        let s = Sessione {
            nome: testa.to_string(),
            host,
            utente,
            ..Default::default()
        };
        return Ok(s);
    }
    let nomi: Vec<String> = sessioni.iter().map(|s| s.nome.clone()).collect();
    Err(format!(
        "«{testa}» non è una sessione salvata{}. Usa «nome:percorso» con una di queste, \
         oppure «utente@host:percorso».",
        if nomi.is_empty() {
            " (non ce n'è nessuna: Ctrl+Maiusc+S per crearne una)".to_string()
        } else {
            format!(" — disponibili: {}", nomi.join(", "))
        }
    ))
}

/// Divide una riga in argomenti rispettando gli apici (`"due parole"`, `'due parole'`).
pub fn dividi_argomenti(riga: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut corrente = String::new();
    let mut apice: Option<char> = None;
    let mut c_escape = false;
    let mut in_parola = false;
    for c in riga.chars() {
        if c_escape {
            corrente.push(c);
            c_escape = false;
            in_parola = true;
            continue;
        }
        match (apice, c) {
            (Some(a), _) if c == a => apice = None,
            (Some(_), '\\') => c_escape = true,
            (Some(_), _) => corrente.push(c),
            (None, '\'') | (None, '"') => {
                apice = Some(c);
                in_parola = true;
            }
            (None, '\\') => c_escape = true,
            (None, c) if c.is_whitespace() => {
                if in_parola {
                    out.push(std::mem::take(&mut corrente));
                    in_parola = false;
                }
            }
            (None, c) => {
                corrente.push(c);
                in_parola = true;
            }
        }
    }
    if apice.is_some() {
        return Err("apice non chiuso nella riga di comando".into());
    }
    if c_escape {
        return Err("la riga finisce con una barra rovesciata".into());
    }
    if in_parola {
        out.push(corrente);
    }
    Ok(out)
}

/// Espande `~` e rende assoluto un percorso locale.
pub fn espandi(p: &str, cwd: Option<&Path>) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let p = p.trim();
    if p.is_empty() {
        return home;
    }
    if p == "~" {
        return home;
    }
    if let Some(resto) = p.strip_prefix("~/") {
        return format!("{home}/{resto}");
    }
    let path = Path::new(p);
    if path.is_absolute() {
        return p.to_string();
    }
    let base: PathBuf = match cwd {
        Some(d) => d.to_path_buf(),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(&home)),
    };
    base.join(path).to_string_lossy().to_string()
}

/// Che cosa c'è nel percorso: `stat` in locale, un `ssh` solo per il remoto.
/// **Bloccante**: chiamarla da un thread.
pub fn rileva(lato: &Lato) -> Result<Info, String> {
    match &lato.remoto {
        None => rileva_locale(&lato.percorso),
        Some(s) => rileva_remoto(s, &lato.percorso),
    }
}

fn rileva_locale(p: &str) -> Result<Info, String> {
    let md = std::fs::metadata(p).map_err(|e| format!("{p}: {e}"))?;
    if md.is_dir() {
        Ok(Info {
            tipo: Tipo::Cartella,
            byte: dimensione_cartella(p),
            elementi: conta_file(p),
        })
    } else {
        Ok(Info { tipo: Tipo::File, byte: md.len(), elementi: 1 })
    }
}

/// Dimensione reale di una cartella con `du -sb` (una riga, niente cicli di link).
fn dimensione_cartella(p: &str) -> u64 {
    let out = Command::new("du").arg("-sb").arg("--").arg(p).output();
    let Ok(out) = out else { return 0 };
    let testo = String::from_utf8_lossy(&out.stdout);
    testo
        .split_whitespace()
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Numero di file (non cartelle) dentro una cartella.
fn conta_file(p: &str) -> u64 {
    let out = Command::new("find").arg(p).arg("-type").arg("f").output();
    let Ok(out) = out else { return 0 };
    out.stdout.iter().filter(|b| **b == b'\n').count() as u64
}

fn rileva_remoto(s: &Sessione, p: &str) -> Result<Info, String> {
    let qp = q_remoto(p);
    // Una sola connessione: dice tipo, dimensione e quanti file ci sono dentro.
    let cmd = format!(
        "if [ -d {qp} ]; then printf 'cartella\\t%s\\t%s\\n' \
         \"$(du -sb -- {qp} 2>/dev/null | cut -f1)\" \
         \"$(find {qp} -type f 2>/dev/null | wc -l)\"; \
         elif [ -f {qp} ]; then printf 'file\\t%s\\t1\\n' \"$(stat -c %s -- {qp} 2>/dev/null)\"; \
         else echo 'manca'; fi"
    );
    let (prog, args) = s.ssh_comando(&cmd, true);
    let out = Command::new(&prog)
        .args(&args)
        .output()
        .map_err(|e| format!("ssh: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() {
            format!("{p}: non raggiungibile con «{}»", s.nome)
        } else {
            err
        });
    }
    let testo = String::from_utf8_lossy(&out.stdout);
    let riga = testo.lines().next().unwrap_or("").trim().to_string();
    if riga == "manca" || riga.is_empty() {
        return Err(format!("{p}: non esiste su «{}»", s.nome));
    }
    let campi: Vec<&str> = riga.split('\t').collect();
    let tipo = match campi.first().copied() {
        Some("cartella") => Tipo::Cartella,
        Some("file") => Tipo::File,
        _ => return Err(format!("{p}: risposta inattesa da «{}»: {riga}", s.nome)),
    };
    Ok(Info {
        tipo,
        byte: campi.get(1).and_then(|n| n.trim().parse().ok()).unwrap_or(0),
        elementi: campi
            .get(2)
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(1),
    })
}

/// Comando `scp` per il trasferimento.
///
/// `-r` solo se la **sorgente** è una cartella (deciso da `rileva`), `-3` se sono
/// remote tutte e due, `-p` per conservare data e permessi.  `-o BatchMode=yes`: una
/// coda in background non può fermarsi a chiedere una password, quindi si usa la
/// chiave e in caso contrario l'errore lo dice.
pub fn comando(sorg: &Lato, dest: &Lato, ricorsivo: bool) -> (String, Vec<String>) {
    let mut args: Vec<String> = Vec::new();
    if sorg.remoto.is_some() && dest.remoto.is_some() {
        args.push("-3".into());
    }
    if ricorsivo {
        args.push("-r".into());
    }
    args.push("-p".into());
    args.push("-o".into());
    args.push("BatchMode=yes".into());
    // Porta e chiave valgono per **tutti** i collegamenti: se le due macchine ne
    // usano di diverse si prende quella della sorgente (e la finestra lo dice).
    let opzioni = match (&sorg.remoto, &dest.remoto) {
        (Some(a), _) => a.opzioni_scp(),
        (None, Some(b)) => b.opzioni_scp(),
        (None, None) => Vec::new(),
    };
    args.extend(opzioni);
    args.push(sorg.spec_scp());
    args.push(dest.spec_scp());
    ("scp".to_string(), args)
}

/// Avviso da mostrare quando le due macchine richiederebbero opzioni diverse.
pub fn avviso_opzioni(sorg: &Lato, dest: &Lato) -> Option<String> {
    let (a, b) = (sorg.remoto.as_ref()?, dest.remoto.as_ref()?);
    if a.porta != b.porta || a.chiave_esistente() != b.chiave_esistente() {
        Some(format!(
            "«{}» e «{}» usano porta o chiave diverse: il trasferimento usa quelle di «{}»",
            a.nome, b.nome, a.nome
        ))
    } else {
        None
    }
}

/// Percentuale da una riga di avanzamento di `scp` («doc.pdf 42% 5120KB …»).
pub fn percentuale(riga: &str) -> Option<u8> {
    let i = riga.find('%')?;
    let cifre: String = riga[..i]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    if cifre.is_empty() {
        None
    } else {
        cifre.parse::<u8>().ok().map(|p| p.min(100))
    }
}

/// Dimensione leggibile in italiano («1,2 GB»).
pub fn formatta_byte(b: u64) -> String {
    const UNITA: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITA.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{:.1} {}", v, UNITA[i]).replace('.', ",")
    }
}

/// Quoting per una shell POSIX.
fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Quoting sul **remoto** che lascia espandere `~` (diventa `$HOME`).
fn q_remoto(p: &str) -> String {
    match p.strip_prefix('~') {
        Some(resto) => format!("$HOME{}", q(resto)),
        None => q(p),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sessioni() -> Vec<Sessione> {
        vec![
            Sessione {
                nome: "207".into(),
                host: "192.0.2.12".into(),
                utente: "utente".into(),
                ..Default::default()
            },
            Sessione {
                nome: "201".into(),
                host: "192.0.2.11".into(),
                utente: "utente".into(),
                porta: 2222,
                chiave: Some("/home/utente/.ssh/id_ed25519".into()),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn analizza_remoto_e_locale() {
        let s = sessioni();
        let l = analizza("207:~/Documenti", &s, None).unwrap();
        assert_eq!(l.remoto.as_ref().unwrap().nome, "207");
        assert_eq!(l.percorso, "~/Documenti");

        let l = analizza("207:", &s, None).unwrap();
        assert_eq!(l.percorso, "~", "senza percorso vale la cartella personale");

        let l = analizza("~/foto.jpg", &s, None).unwrap();
        assert!(l.remoto.is_none());
        assert!(l.percorso.ends_with("/foto.jpg"));

        // «utente@host:percorso» senza sessione salvata
        let l = analizza("root@10.0.0.5:/etc", &s, None).unwrap();
        assert_eq!(l.remoto.as_ref().unwrap().host, "10.0.0.5");
        assert_eq!(l.remoto.as_ref().unwrap().utente, "root");
        assert_eq!(l.remoto.as_ref().unwrap().porta, 22);

        // sconosciuto: errore con l'elenco
        let e = analizza("pippo:~/x", &s, None).unwrap_err();
        assert!(e.contains("207"), "{e}");

        // i due punti dopo una barra non separano: percorso locale
        let l = analizza("./strano:nome", &s, None).unwrap();
        assert!(l.remoto.is_none());
    }

    #[test]
    fn dividi_riga_con_apici() {
        assert_eq!(
            dividi_argomenti("~/a.pdf 207:~/Documenti/").unwrap(),
            vec!["~/a.pdf", "207:~/Documenti/"]
        );
        assert_eq!(
            dividi_argomenti(r#""la mia foto.jpg" 207:/tmp/"#).unwrap(),
            vec!["la mia foto.jpg", "207:/tmp/"]
        );
        assert_eq!(
            dividi_argomenti("'/tmp/due parole' 201:~/").unwrap(),
            vec!["/tmp/due parole", "201:~/"]
        );
        assert!(dividi_argomenti("'non chiuso").is_err());
    }

    #[test]
    fn comando_locale_verso_remoto() {
        let s = sessioni();
        let sorg = Lato::locale("/tmp/foto.jpg", None);
        let dest = analizza("207:~/Immagini/", &s, None).unwrap();
        let (prog, args) = comando(&sorg, &dest, false);
        assert_eq!(prog, "scp");
        assert!(!args.contains(&"-r".to_string()), "un file non è ricorsivo");
        assert!(!args.contains(&"-3".to_string()), "un lato è locale");
        assert!(args.contains(&"-p".to_string()));
        assert_eq!(args.last().unwrap(), "utente@192.0.2.12:~/Immagini/");
        assert_eq!(args[args.len() - 2], "/tmp/foto.jpg");
    }

    #[test]
    fn comando_cartella_e_remoto_remoto() {
        let s = sessioni();
        let sorg = analizza("207:/var/log", &s, None).unwrap();
        let dest = analizza("201:~/logs/", &s, None).unwrap();
        let (_, args) = comando(&sorg, &dest, true);
        assert!(args.contains(&"-r".to_string()), "cartella -> -r");
        assert!(args.contains(&"-3".to_string()), "remoto -> remoto passa da qui");
        // opzioni: quelle della sorgente (porta 22, nessuna chiave indicata)
        assert!(args.contains(&"-P".to_string()));
        assert!(avviso_opzioni(&sorg, &dest).is_some(), "porte diverse: si avvisa");
    }

    #[test]
    fn percentuale_dalle_righe_di_scp() {
        assert_eq!(percentuale("foto.jpg    42%  5120KB  10.2MB/s   00:03 ETA"), Some(42));
        assert_eq!(percentuale("       100%  1,2MB  done"), Some(100));
        assert_eq!(percentuale("niente da leggere"), None);
        assert_eq!(percentuale("% senza cifre"), None);
    }

    #[test]
    fn dimensioni_leggibili() {
        assert_eq!(formatta_byte(999), "999 B");
        assert_eq!(formatta_byte(1_500), "1,5 kB");
        assert_eq!(formatta_byte(12_400_000), "12,4 MB");
        assert_eq!(formatta_byte(1_234_567_890), "1,2 GB");
    }

    #[test]
    fn rileva_un_file_e_una_cartella_veri() {
        let dir = std::env::temp_dir().join(format!("zirkio-scp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("dentro")).unwrap();
        std::fs::write(dir.join("a.txt"), b"ciao").unwrap();
        std::fs::write(dir.join("dentro/b.txt"), b"mondo!").unwrap();

        let f = rileva(&Lato::locale(&dir.join("a.txt").to_string_lossy(), None)).unwrap();
        assert_eq!(f.tipo, Tipo::File);
        assert_eq!(f.byte, 4);

        let c = rileva(&Lato::locale(&dir.to_string_lossy(), None)).unwrap();
        assert_eq!(c.tipo, Tipo::Cartella);
        assert_eq!(c.elementi, 2, "due file dentro");
        assert!(c.byte > 0);
        assert!(c.descrizione().starts_with("cartella · 2 file · "));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
