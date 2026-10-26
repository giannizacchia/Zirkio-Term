//! Pseudo-terminale: si apre una `pty`, ci si lancia dentro la shell e si pompa
//! l'I/O su due thread (lettura e scrittura) così il thread grafico non si blocca mai.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;

/// Cerca una voce del database terminfo, sia nella forma «a cartelle»
/// (`<base>/x/xterm-direct`) sia in quella con l'iniziale in esadecimale
/// (`<base>/78/xterm-direct`), e guarda anche `$TERMINFO`, `$TERMINFO_DIRS`
/// e `~/.terminfo`.
fn voce_terminfo(nome: &str) -> bool {
    let iniziale = &nome[..1];
    let a_cartelle = format!("{iniziale}/{nome}");
    let esadecimale = format!("{:x}/{nome}", iniziale.as_bytes()[0]);

    let mut basi: Vec<PathBuf> = Vec::new();
    if let Some(t) = std::env::var_os("TERMINFO") {
        basi.push(PathBuf::from(t));
    }
    if let Some(t) = std::env::var_os("TERMINFO_DIRS") {
        basi.extend(std::env::split_paths(&t));
    }
    if let Some(h) = std::env::var_os("HOME") {
        basi.push(PathBuf::from(h).join(".terminfo"));
    }
    for b in ["/usr/share/terminfo", "/usr/lib/terminfo", "/lib/terminfo", "/etc/terminfo"] {
        basi.push(PathBuf::from(b));
    }

    basi.iter().any(|b| b.join(&a_cartelle).is_file() || b.join(&esadecimale).is_file())
}

/// Nome di `TERM` da dichiarare ai programmi che girano dentro il terminale.
///
/// `xterm-direct` dice la verità — qui il colore a 24 bit funziona — ed è quello
/// che cercano i programmi per attivare il truecolor (per esempio `mc` carica le
/// skin truecolor solo con un `TERM` `*-direct*`). Se però sul sistema manca la
/// voce terminfo si resta su `xterm-256color`: un `TERM` senza voce manda in tilt
/// curses.
pub fn nome_term() -> &'static str {
    if voce_terminfo("xterm-direct") {
        "xterm-direct"
    } else {
        "xterm-256color"
    }
}

pub struct Pty {
    master: OwnedFd,
    child: Child,
    writer: Option<Sender<Vec<u8>>>,
    /// La sessione è già stata chiusa con `termina` (non si ripete).
    terminato: bool,
}

/// Chi riceve i byte del processo e la sua fine.
pub struct Sink {
    pub on_data: Box<dyn FnMut(Vec<u8>) + Send>,
    pub on_exit: Box<dyn FnMut() + Send>,
}

/// Uso della sessione da **un altro thread** (vedi [`Pty::maniglia`]).
///
/// L'fd è una `dup` del master, tenuta viva da un `Arc`: chi la usa può aspettare
/// senza bloccare la grafica e senza rischiare che il pannello la chiuda sotto i piedi.
#[derive(Clone)]
pub struct Maniglia {
    fd: std::sync::Arc<OwnedFd>,
    tx: Sender<Vec<u8>>,
}

impl Maniglia {
    /// Descrittore del master (per `tcgetpgrp`, `tcgetattr`).
    pub fn fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Scrive nella sessione (passa per la coda del thread di scrittura).
    pub fn scrivi(&self, dati: &[u8]) {
        if !dati.is_empty() {
            let _ = self.tx.send(dati.to_vec());
        }
    }
}

impl Pty {
    pub fn spawn(
        shell: &str,
        args: &[String],
        cols: usize,
        rows: usize,
        cwd: Option<&Path>,
        mut sink: Sink,
    ) -> io::Result<Pty> {
        let ws = libc::winsize {
            ws_row: rows.max(1) as u16,
            ws_col: cols.max(1) as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let mut master: libc::c_int = -1;
        let mut slave: libc::c_int = -1;
        let rc = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &ws,
            )
        };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // Il master non deve finire nel processo figlio.
        unsafe {
            libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        let master = unsafe { OwnedFd::from_raw_fd(master) };

        let stdin_fd = slave;
        let stdout_fd = unsafe { libc::dup(slave) };
        let stderr_fd = unsafe { libc::dup(slave) };
        if stdout_fd < 0 || stderr_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let stdin_file = unsafe { std::fs::File::from_raw_fd(stdin_fd) };
        let stdout_file = unsafe { std::fs::File::from_raw_fd(stdout_fd) };
        let stderr_file = unsafe { std::fs::File::from_raw_fd(stderr_fd) };

        let mut cmd = Command::new(shell);
        cmd.args(args)
            .stdin(Stdio::from(stdin_file))
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file))
            .env("TERM", nome_term())
            .env("COLORTERM", "truecolor")
            .env("TERM_PROGRAM", "zirkio-term")
            .env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"))
            .env("ZIRKIO_TERM", "1");
        // Se l'app è partita senza locale (autostart, lanciatore, sessione grafica senza
        // LANG), i programmi a schermo intero degradano: `mc` disegna i riquadri con le
        // lettere ACS invece delle linee (segnalato dall'utente il 24/10).  Si mette un
        // locale UTF-8 **solo** quando manca del tutto.
        let senza_locale = |v: &str| std::env::var_os(v).map(|x| x.is_empty()).unwrap_or(true);
        if senza_locale("LC_ALL") && senza_locale("LANG") {
            cmd.env("LANG", "C.UTF-8").env("LC_CTYPE", "C.UTF-8");
        }
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        } else if let Some(home) = std::env::var_os("HOME") {
            cmd.current_dir(home);
        }
        unsafe {
            cmd.pre_exec(|| {
                // Sessione nuova e terminale di controllo: la shell si comporta da login.
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);
                Ok(())
            });
        }
        let child = cmd.spawn()?;

        // Lettura: thread dedicato, buffer riusato.
        let read_fd: RawFd = unsafe { libc::dup(master.as_raw_fd()) };
        if read_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut reader = unsafe { std::fs::File::from_raw_fd(read_fd) };
        thread::Builder::new()
            .name("zirkio-pty-read".into())
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => (sink.on_data)(buf[..n].to_vec()),
                        Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
                (sink.on_exit)();
            })?;

        // Scrittura: thread dedicato, così un processo lento non blocca la grafica.
        let write_fd: RawFd = unsafe { libc::dup(master.as_raw_fd()) };
        if write_fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut writer_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        thread::Builder::new()
            .name("zirkio-pty-write".into())
            .spawn(move || {
                while let Ok(mut data) = rx.recv() {
                    while let Ok(more) = rx.try_recv() {
                        data.extend_from_slice(&more);
                    }
                    if writer_file.write_all(&data).is_err() {
                        break;
                    }
                }
            })?;

        Ok(Pty { master, child, writer: Some(tx), terminato: false })
    }

    pub fn write(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        if let Some(tx) = &self.writer {
            let _ = tx.send(data.to_vec());
        }
    }

    /// Maniglia alla sessione per un **altro thread**: l'fd del master duplicato
    /// (così vive anche se il pannello viene chiuso) e la coda di scrittura.
    ///
    /// Serve all'inserimento automatico della password di sudo, che deve poter
    /// aspettare (`tcgetattr`, `/proc`) senza fermare il ciclo grafico.
    pub fn maniglia(&self) -> Option<Maniglia> {
        let tx = self.writer.clone()?;
        let fd = unsafe { libc::dup(self.master.as_raw_fd()) };
        if fd < 0 {
            return None;
        }
        Some(Maniglia {
            fd: std::sync::Arc::new(unsafe { OwnedFd::from_raw_fd(fd) }),
            tx,
        })
    }

    pub fn resize(&self, cols: usize, rows: usize) {
        let ws = libc::winsize {
            ws_row: rows.max(1) as u16,
            ws_col: cols.max(1) as u16,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws);
        }
    }

    pub fn pid(&self) -> i32 {
        self.child.id() as i32
    }

    /// `true` se il processo è terminato.
    pub fn try_wait(&mut self) -> io::Result<Option<i32>> {
        Ok(self.child.try_wait()?.map(|s| s.code().unwrap_or(-1)))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Chiude la sessione **con garbo**: manda SIGHUP perché la shell esca da sola e
    /// scriva il file di cronologia (con SIGKILL la cronologia va persa), e chiude la
    /// nostra copia del master.  Idempotente: la chiama anche `Drop`.
    pub fn termina(&mut self) {
        if self.terminato {
            return;
        }
        self.terminato = true;
        self.writer.take();
        // chiude la copia "principale" del master: se i thread di lettura/scrittura hanno
        // già finito, il pty si chiude del tutto e la shell riceve comunque SIGHUP.
        let fds = unsafe { libc::dup(std::io::stdin().as_raw_fd()) };
        if fds >= 0 {
            let inerte = unsafe { OwnedFd::from_raw_fd(fds) };
            let vecchio = std::mem::replace(&mut self.master, inerte);
            drop(vecchio);
        }
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGHUP);
        }
        if !attendi(&mut self.child, 250) {
            unsafe {
                libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
            }
            if !attendi(&mut self.child, 150) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.termina();
    }
}

#[allow(dead_code)]
impl Pty {
    fn drop_lento(&mut self) {
        // Chiudendo la parte writer e il **master** la shell riceve SIGHUP: esce da sola e
        // — importante per zsh/bash — **salva il file di cronologia**.  Prima si mandava
        // subito `Child::kill()` (SIGKILL): la shell moriva sul colpo e i comandi digitati
        // non finivano mai nella cronologia (segnalato dall'utente il 25/10).
        self.writer.take();
        // il master è un `OwnedFd`: non si può "chiudere" senza muoverlo, quindi si
        // sostituisce con un fd inerte e si lascia cadere il vecchio.
        let fds = unsafe { libc::dup(std::io::stdin().as_raw_fd()) };
        let inerte = unsafe { OwnedFd::from_raw_fd(fds) };
        let _master = std::mem::replace(&mut self.master, inerte);
        drop(_master);

        // un attimo per l'uscita pulita (zsh scrive la cronologia qui)
        if attendi(&mut self.child, 200) {
            traccia("uscita da sola dopo la chiusura del master");
            return;
        }
        // non è uscita: SIGHUP/SIGTERM espliciti
        traccia("mando SIGHUP");
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGHUP);
        }
        if attendi(&mut self.child, 150) {
            traccia("uscita dopo SIGHUP");
            return;
        }
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }
        if attendi(&mut self.child, 150) {
            traccia("uscita dopo SIGTERM");
            return;
        }
        traccia("SIGKILL: la shell non è uscita");
        let _ = self.child.kill(); // SIGKILL solo come ultima risorsa
        let _ = self.child.wait();
    }
}

/// Traccia diagnostica (solo con `ZIRKIO_STATS`).
fn traccia(msg: &str) {
    if std::env::var_os("ZIRKIO_STATS").is_some() {
        eprintln!("[stats] pty drop: {msg}");
    }
}

/// Aspetta che il figlio esca, al massimo `ms` millisecondi.  `true` se è uscito.
fn attendi(child: &mut Child, ms: u64) -> bool {
    let scadenza = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => {}
            Err(_) => return true,
        }
        if std::time::Instant::now() >= scadenza {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Cartella corrente di un processo (`/proc/<pid>/cwd`).
///
/// Serve a `@scp`: un percorso relativo digitato nel terminale va inteso rispetto alla
/// cartella della **shell**, non a quella con cui è partita l'app.
pub fn cwd_di(pid: i32) -> Option<std::path::PathBuf> {
    if pid <= 0 {
        return None;
    }
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

/// Comandi di sistema utili al terminale (aprire un link, leggere la clipboard).
pub fn open_url(url: &str) {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return;
    }
    let _ = Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voce_terminfo_cerca_nel_database() {
        let base = std::env::temp_dir().join(format!("zirkio-term-ti-{}", std::process::id()));
        let voce = base.join("78").join("xterm-direct"); // forma con l'iniziale in esadecimale
        std::fs::create_dir_all(voce.parent().unwrap()).unwrap();
        std::fs::write(&voce, b"").unwrap();

        let vecchio = std::env::var_os("TERMINFO");
        unsafe { std::env::set_var("TERMINFO", &base) };
        assert!(voce_terminfo("xterm-direct"), "la voce finta deve essere trovata");
        assert!(!voce_terminfo("zirkio-inesistente"));

        match vecchio {
            Some(v) => unsafe { std::env::set_var("TERMINFO", v) },
            None => unsafe { std::env::remove_var("TERMINFO") },
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn nome_term_dichiara_solo_voci_vere() {
        // Sul sistema puo' esserci o non esserci xterm-direct: in entrambi i casi
        // il nome scelto deve essere una voce terminfo esistente.
        let nome = nome_term();
        assert!(voce_terminfo(nome), "TERM={nome} non e' nel database terminfo");
        assert!(nome == "xterm-direct" || nome == "xterm-256color");
    }
}
