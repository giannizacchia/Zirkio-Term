//! Configurazione: `~/.config/zirkio-term/zirkio-term.toml`.
//!
//! Il file è **facoltativo**: senza file si parte con i valori di questa pagina.

use std::path::PathBuf;

use serde::Deserialize;

use crate::term::{Rgb, palette_256};

#[derive(Debug, Clone)]
pub struct Config {
    pub font: FontConfig,
    pub window: WindowConfig,
    pub theme: Theme,
    pub scrollback: usize,
    pub shell: Option<String>,
    pub shell_args: Vec<String>,
    pub padding: u32,
    /// Password di sudo: inserimento automatico e dove si tiene.
    pub sudo: SudoConfig,
}

/// Inserimento automatico della password quando `sudo` la chiede.
///
/// `inserimento` è la funzione; `ricorda` dice **dove** sta la password: in
/// memoria per la sessione dell'app (predefinito), oppure nel portachiavi di
/// sistema.  Sul disco non finisce mai.
#[derive(Debug, Clone)]
pub struct SudoConfig {
    pub inserimento: bool,
    pub ricorda: bool,
}

impl Default for SudoConfig {
    fn default() -> Self {
        SudoConfig {
            inserimento: false,
            // «la prima volta la chiede e poi la salva»: il salvataggio è la scelta
            // di partenza, si toglie dalle preferenze.
            ricorda: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FontConfig {
    /// `None` = si cerca un monospazio di sistema.
    pub family: Option<String>,
    pub size: f32,
    /// Altezza riga come multiplo dell'altezza del carattere.
    pub line_height: f32,
    /// Spessore del grassetto finto quando il font non ha il neretto.
    pub bold_strength: f32,
}

#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
    pub title: String,
    pub maximized: bool,
    pub opacity: f32,
}

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub palette: [Rgb; 16],
    pub ui_bg: Rgb,
    pub ui_fg: Rgb,
    pub ui_dim: Rgb,
    pub accent: Rgb,
}

impl Theme {
    pub fn dark() -> Theme {
        Theme {
            name: "Zirkio Dark".into(),
            fg: Rgb::new(0xd8, 0xde, 0xe9),
            bg: Rgb::new(0x11, 0x13, 0x19),
            cursor: Rgb::new(0x7a, 0xa2, 0xf7),
            selection: Rgb::new(0x2b, 0x35, 0x55),
            palette: [
                Rgb::new(0x1c, 0x1f, 0x27),
                Rgb::new(0xe0, 0x6c, 0x75),
                Rgb::new(0x9e, 0xce, 0x6a),
                Rgb::new(0xe5, 0xc0, 0x7b),
                Rgb::new(0x7a, 0xa2, 0xf7),
                Rgb::new(0xbb, 0x9a, 0xf7),
                Rgb::new(0x7d, 0xcf, 0xff),
                Rgb::new(0xa9, 0xb1, 0xd6),
                Rgb::new(0x41, 0x48, 0x68),
                Rgb::new(0xf7, 0x76, 0x8e),
                Rgb::new(0xb9, 0xf2, 0x7c),
                Rgb::new(0xff, 0xd1, 0x7a),
                Rgb::new(0x8c, 0xb4, 0xff),
                Rgb::new(0xc9, 0x9c, 0xff),
                Rgb::new(0xa4, 0xe0, 0xff),
                Rgb::new(0xe6, 0xea, 0xf5),
            ],
            ui_bg: Rgb::new(0x18, 0x1b, 0x23),
            ui_fg: Rgb::new(0xc5, 0xcc, 0xdd),
            ui_dim: Rgb::new(0x6b, 0x74, 0x8c),
            accent: Rgb::new(0x7a, 0xa2, 0xf7),
        }
    }

    pub fn light() -> Theme {
        Theme {
            name: "Zirkio Light".into(),
            fg: Rgb::new(0x24, 0x29, 0x2f),
            bg: Rgb::new(0xfb, 0xfb, 0xfd),
            cursor: Rgb::new(0x2f, 0x6f, 0xdb),
            selection: Rgb::new(0xcf, 0xdf, 0xf7),
            palette: [
                Rgb::new(0x24, 0x29, 0x2f),
                Rgb::new(0xb3, 0x1d, 0x28),
                Rgb::new(0x11, 0x66, 0x1f),
                Rgb::new(0x8a, 0x63, 0x00),
                Rgb::new(0x1b, 0x51, 0xb0),
                Rgb::new(0x7c, 0x2d, 0xa8),
                Rgb::new(0x0b, 0x6c, 0x8a),
                Rgb::new(0x6a, 0x70, 0x7a),
                Rgb::new(0x51, 0x57, 0x61),
                Rgb::new(0xd1, 0x24, 0x2f),
                Rgb::new(0x1a, 0x7f, 0x37),
                Rgb::new(0x9a, 0x6a, 0x00),
                Rgb::new(0x09, 0x69, 0xdb),
                Rgb::new(0x82, 0x50, 0xdf),
                Rgb::new(0x1b, 0x7f, 0x8f),
                Rgb::new(0x2b, 0x30, 0x38),
            ],
            ui_bg: Rgb::new(0xf1, 0xf2, 0xf5),
            ui_fg: Rgb::new(0x3a, 0x40, 0x4a),
            ui_dim: Rgb::new(0x8a, 0x91, 0x9c),
            accent: Rgb::new(0x09, 0x69, 0xdb),
        }
    }

    /// Valori a 256 colori calcolati dalla tavolozza di 16.
    pub fn palette256(&self) -> [Rgb; 256] {
        palette_256(self.palette)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            font: FontConfig {
                // Carattere di partenza: JetBrains Mono (installato dall'utente in
                // ~/.local/share/fonts/zirkio).  Se manca, `fc-match` ripiega su un
                // monospazio di sistema.
                family: Some("JetBrains Mono".into()),
                size: 13.0,
                line_height: 1.12,
                bold_strength: 0.35,
            },
            window: WindowConfig {
                width: 1060,
                height: 680,
                title: "zirkio-term".into(),
                maximized: false,
                opacity: 1.0,
            },
            theme: Theme::dark(),
            scrollback: 10_000,
            shell: None,
            shell_args: Vec::new(),
            padding: 8,
            sudo: SudoConfig::default(),
        }
    }
}

// ---------------------------------------------------------------- file TOML

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    font: RawFont,
    #[serde(default)]
    window: RawWindow,
    #[serde(default)]
    theme: RawTheme,
    scrollback: Option<usize>,
    shell: Option<String>,
    #[serde(default)]
    shell_args: Vec<String>,
    padding: Option<u32>,
    #[serde(default)]
    sudo: RawSudo,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSudo {
    inserimento: Option<bool>,
    ricorda: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFont {
    family: Option<String>,
    size: Option<f32>,
    line_height: Option<f32>,
    bold_strength: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    width: Option<u32>,
    height: Option<u32>,
    title: Option<String>,
    maximized: Option<bool>,
    opacity: Option<f32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTheme {
    /// `dark` oppure `light`.
    name: Option<String>,
    fg: Option<String>,
    bg: Option<String>,
    cursor: Option<String>,
    selection: Option<String>,
    palette: Option<Vec<String>>,
    ui_bg: Option<String>,
    ui_fg: Option<String>,
    accent: Option<String>,
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        if let Ok(p) = std::env::var("ZIRKIO_TERM_CONFIG") {
            return Some(PathBuf::from(p));
        }
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .ok()
            .or_else(|| std::env::var("HOME").ok().map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("zirkio-term").join("zirkio-term.toml"))
    }

    pub fn load() -> Result<Config, String> {
        let mut cfg = Config::default();
        let Some(path) = Config::path() else {
            return Ok(cfg);
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(cfg),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        // Un file scritto male non deve impedire l'avvio: si segnala e si parte con i
        // valori predefiniti (prima l'app usciva subito e, lanciata dal menu, sembrava
        // non fare niente).
        match toml::from_str::<RawConfig>(&text) {
            Ok(raw) => cfg.merge(raw),
            Err(e) => {
                eprintln!(
                    "zirkio-term: {}: {e} — uso la configurazione predefinita",
                    path.display()
                );
            }
        }
        Ok(cfg)
    }

    fn merge(&mut self, raw: RawConfig) {
        if let Some(v) = raw.font.family {
            self.font.family = Some(v);
        }
        if let Some(v) = raw.font.size {
            self.font.size = v.clamp(6.0, 72.0);
        }
        if let Some(v) = raw.font.line_height {
            self.font.line_height = v.clamp(1.0, 2.5);
        }
        if let Some(v) = raw.font.bold_strength {
            self.font.bold_strength = v.clamp(0.0, 2.0);
        }
        if let Some(v) = raw.window.width {
            self.window.width = v.max(200);
        }
        if let Some(v) = raw.window.height {
            self.window.height = v.max(120);
        }
        if let Some(v) = raw.window.title {
            self.window.title = v;
        }
        if let Some(v) = raw.window.maximized {
            self.window.maximized = v;
        }
        if let Some(v) = raw.window.opacity {
            self.window.opacity = v.clamp(0.3, 1.0);
        }
        if let Some(v) = raw.scrollback {
            self.scrollback = v.min(500_000);
        }
        if let Some(v) = raw.shell {
            self.shell = Some(v);
        }
        if !raw.shell_args.is_empty() {
            self.shell_args = raw.shell_args;
        }
        if let Some(v) = raw.padding {
            self.padding = v.min(64);
        }
        if let Some(v) = raw.sudo.inserimento {
            self.sudo.inserimento = v;
        }
        if let Some(v) = raw.sudo.ricorda {
            self.sudo.ricorda = v;
        }

        let t = &mut self.theme;
        if let Some(name) = raw.theme.name.as_deref() {
            *t = match name.to_ascii_lowercase().as_str() {
                "light" | "chiaro" => Theme::light(),
                _ => Theme::dark(),
            };
        }
        let mut set = |dst: &mut Rgb, src: &Option<String>| {
            if let Some(s) = src {
                if let Some(c) = Rgb::from_hex(s) {
                    *dst = c;
                }
            }
        };
        set(&mut t.fg, &raw.theme.fg);
        set(&mut t.bg, &raw.theme.bg);
        set(&mut t.cursor, &raw.theme.cursor);
        set(&mut t.selection, &raw.theme.selection);
        set(&mut t.ui_bg, &raw.theme.ui_bg);
        set(&mut t.ui_fg, &raw.theme.ui_fg);
        set(&mut t.accent, &raw.theme.accent);
        if let Some(p) = raw.theme.palette {
            for (i, hex) in p.iter().take(16).enumerate() {
                if let Some(c) = Rgb::from_hex(hex) {
                    t.palette[i] = c;
                }
            }
        }
        drop(t);
        self.normalizza();
    }

    /// Riporta i valori dentro limiti sensati.  Serve perché la configurazione può
    /// arrivare da un file scritto a mano (o da una versione futura): un `size = 500`
    /// non deve far esplodere la memoria del disegno.
    pub fn normalizza(&mut self) {
        self.font.size = self.font.size.clamp(6.0, 72.0);
        self.font.line_height = self.font.line_height.clamp(1.0, 2.5);
        self.font.bold_strength = self.font.bold_strength.clamp(0.0, 1.0);
        self.padding = self.padding.min(64);
        self.scrollback = self.scrollback.clamp(100, 500_000);
        self.window.opacity = self.window.opacity.clamp(0.3, 1.0);
        self.window.width = self.window.width.max(240);
        self.window.height = self.window.height.max(160);
    }

    /// Shell da lanciare: quella configurata, altrimenti quella di login dell'utente.
    pub fn shell_command(&self) -> (String, Vec<String>) {
        if let Some(shell) = &self.shell {
            return (shell.clone(), self.shell_args.clone());
        }
        let from_passwd = std::fs::read_to_string("/etc/passwd").ok().and_then(|text| {
            let user = std::env::var("USER").or_else(|_| std::env::var("LOGNAME")).ok()?;
            text.lines()
                .find(|l| l.starts_with(&format!("{user}:")))
                .and_then(|l| l.split(':').nth(6))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        });
        let shell = from_passwd
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".to_string());
        (shell, self.shell_args.clone())
    }
}

// ---------------------------------------------------------------- scrittura

impl Config {
    /// Scrive la configurazione nel file TOML (crea la cartella se serve).
    ///
    /// Scritta a mano invece che con un serializzatore: sono pochi campi e così
    /// il file resta leggibile e commentato.
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Config::path().ok_or("percorso di configurazione non determinabile")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let hex = |c: Rgb| format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2);
        let mut out = String::new();
        out.push_str("# zirkio-term — configurazione (scritta dalla finestra Configurazione)\n\n");
        // ⚠️ I campi di primo livello vanno scritti PRIMA della prima sezione `[nome]`:
        // in TOML tutto ciò che segue un'intestazione appartiene a quella tabella, e
        // `scrollback`/`padding` scritti dentro `[window]` rendevano il file invalido
        // (errore di lettura all'avvio successivo: l'app non si apriva più).
        out.push_str(&format!("scrollback = {}\npadding = {}\n", self.scrollback, self.padding));
        if let Some(sh) = &self.shell {
            out.push_str(&format!("shell = \"{}\"\n", q(sh)));
        }
        out.push('\n');
        out.push_str("[font]\n");
        if let Some(f) = &self.font.family {
            out.push_str(&format!("family = \"{}\"\n", q(f)));
        }
        out.push_str(&format!("size = {}\n", self.font.size));
        out.push_str(&format!("line_height = {}\n", self.font.line_height));
        out.push_str(&format!("bold_strength = {}\n\n", self.font.bold_strength));
        out.push_str("[window]\n");
        out.push_str(&format!("width = {}\n", self.window.width));
        out.push_str(&format!("height = {}\n", self.window.height));
        out.push_str(&format!("opacity = {}\n", self.window.opacity));
        out.push_str(&format!("maximized = {}\n\n", if self.window.maximized { "true" } else { "false" }));
        out.push_str("[theme]\n");
        out.push_str(&format!(
            "name = \"{}\"\nfg = \"{}\"\nbg = \"{}\"\ncursor = \"{}\"\nselection = \"{}\"\n",
            if self.theme.name.to_ascii_lowercase().contains("light") { "light" } else { "dark" },
            hex(self.theme.fg),
            hex(self.theme.bg),
            hex(self.theme.cursor),
            hex(self.theme.selection)
        ));
        let pal: Vec<String> = self.theme.palette.iter().map(|c| format!("\"{}\"", hex(*c))).collect();
        out.push_str(&format!("palette = [{}]\n\n", pal.join(", ")));
        out.push_str("[sudo]\n");
        out.push_str(&format!(
            "inserimento = {}\nricorda = {}\n",
            self.sudo.inserimento, self.sudo.ricorda
        ));
        std::fs::write(&path, out).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_e_colori() {
        assert_eq!(Rgb::from_hex("#ff0080"), Some(Rgb::new(255, 0, 128)));
        assert_eq!(Rgb::from_hex("00ff00"), Some(Rgb::new(0, 255, 0)));
        assert_eq!(Rgb::from_hex("#xyz"), None);
    }

    #[test]
    fn tavolozza_256() {
        let t = Theme::dark();
        let p = t.palette256();
        assert_eq!(p[0], t.palette[0]);
        assert_eq!(p[255], Rgb::new(238, 238, 238));
        assert_eq!(p[16], Rgb::new(0, 0, 0));
    }

    #[test]
    fn toml_parziale_non_rompe_il_resto() {
        let raw: RawConfig = toml::from_str("[font]\nsize = 16.0\n").unwrap();
        let mut cfg = Config::default();
        cfg.merge(raw);
        assert_eq!(cfg.font.size, 16.0);
        assert_eq!(cfg.scrollback, 10_000);
    }

    /// I due test che usano `ZIRKIO_TERM_CONFIG` non devono sovrapporsi: `cargo test`
    /// li esegue in parallelo e la variabile d'ambiente è una sola per processo.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn salva_e_rilegge_tutti_i_campi() {
        // Il file scritto da `save()` deve poter essere riletto: un campo di primo
        // livello finito dentro una sezione rendeva il TOML invalido e l'app non
        // partiva più (difetto vero del 24/10/2026).
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zirkio-term-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zirkio-term.toml");
        unsafe { std::env::set_var("ZIRKIO_TERM_CONFIG", &path) };
        let mut cfg = Config::default();
        cfg.scrollback = 12_345;
        cfg.padding = 11;
        cfg.font.size = 15.5;
        cfg.theme = Theme::light();
        cfg.shell = Some("/bin/zsh".into());
        cfg.save().expect("scrittura");
        let testo = std::fs::read_to_string(&path).unwrap();
        // Il file deve essere TOML valido per lo schema vero.
        let raw: RawConfig = toml::from_str(&testo).expect("il file scritto deve essere valido");
        let mut back = Config::default();
        back.merge(raw);
        assert_eq!(back.scrollback, 12_345, "scrollback perso");
        assert_eq!(back.padding, 11, "padding perso");
        assert_eq!(back.font.size, 15.5);
        assert_eq!(back.shell.as_deref(), Some("/bin/zsh"));
        assert!(back.theme.name.contains("Light"));
        let letto = Config::load().expect("rilettura");
        assert_eq!(letto.scrollback, 12_345);
        assert!(
            testo.find("scrollback").unwrap() < testo.find("[font]").unwrap(),
            "`scrollback` deve stare prima della prima sezione: {testo}"
        );
        unsafe { std::env::remove_var("ZIRKIO_TERM_CONFIG") };
    }

    #[test]
    fn la_sezione_sudo_si_salva_e_si_rilegge() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zirkio-term-sudo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zirkio-term.toml");
        unsafe { std::env::set_var("ZIRKIO_TERM_CONFIG", &path) };
        let mut cfg = Config::default();
        assert!(!cfg.sudo.inserimento, "l'inserimento automatico è spento di default");
        assert!(
            cfg.sudo.ricorda,
            "di default la password si salva («poi la salva»)"
        );
        cfg.sudo.inserimento = true;
        cfg.sudo.ricorda = true;
        cfg.save().expect("scrittura");
        let letto = Config::load().expect("rilettura");
        assert!(letto.sudo.inserimento);
        assert!(letto.sudo.ricorda);
        unsafe { std::env::remove_var("ZIRKIO_TERM_CONFIG") };
    }

    #[test]
    fn file_rotto_non_impedisce_l_avvio() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("zirkio-term-rotto-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zirkio-term.toml");
        std::fs::write(&path, "[window]\nscrollback = 5\n").unwrap();
        unsafe { std::env::set_var("ZIRKIO_TERM_CONFIG", &path) };
        let cfg = Config::load().expect("un file rotto non deve far uscire l'app");
        assert_eq!(cfg.scrollback, 10_000, "si parte dai valori predefiniti");
        unsafe { std::env::remove_var("ZIRKIO_TERM_CONFIG") };
    }
}
