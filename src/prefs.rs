//! Finestra **Configurazione**: tutte le impostazioni, applicate mentre si cambiano.
//!
//! Le famiglie di caratteri si leggono da `fc-list` (fontconfig è già una dipendenza
//! di fatto: la usa `fc-match` per caricare il font), così l'elenco è sempre quello
//! dei caratteri davvero installati sulla macchina.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::app::App;
use crate::config::{Config, Theme};
use crate::term::CursorStyle;

thread_local! {
    static APERTO: Cell<bool> = const { Cell::new(false) };
}

/// Famiglie monospaziate installate, con le preferite in cima.
fn famiglie_mono() -> Vec<String> {
    let Ok(out) = std::process::Command::new("fc-list")
        .args([":spacing=mono", "family"])
        .output()
    else {
        return vec!["monospace".to_string()];
    };
    let testo = String::from_utf8_lossy(&out.stdout);
    let mut v: Vec<String> = testo
        .lines()
        .flat_map(|l| l.split(','))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| {
            let l = format!(" {} ", s.to_ascii_lowercase());
            [
                " condensed", " expanded", " thin", " extralight", " semibold", " extrabold",
                " medium", " light", " retina", " black", " nerd font",
            ]
            .iter()
            .all(|brutto| !l.contains(brutto))
                // le varianti "NL" e "NF" sono famiglie finte: lasciamo la principale
                || l.contains("nerd font mono")
        })
        .collect();
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup();
    let mut pref: Vec<String> = Vec::new();
    for p in [
        "JetBrains Mono",
        "Fira Code",
        "Hack",
        "MesloLGS NF",
        "Inconsolata",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "monospace",
    ] {
        if let Some(i) = v.iter().position(|f| f == p) {
            pref.push(v.remove(i));
        }
    }
    pref.extend(v);
    pref
}

/// Etichetta di una sezione.
fn sezione(titolo: &str) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_markup(&format!("<b>{titolo}</b>"));
    l.set_xalign(0.0);
    l.set_margin_top(10);
    l.set_margin_bottom(4);
    l
}

/// Riga "etichetta + controllo" dentro una griglia.
fn riga(griglia: &gtk::Grid, r: i32, testo: &str, controllo: &impl IsA<gtk::Widget>) {
    let l = gtk::Label::new(Some(testo));
    l.set_xalign(0.0);
    l.set_margin_end(12);
    griglia.attach(&l, 0, r, 1, 1);
    griglia.attach(controllo, 1, r, 1, 1);
}

pub fn apri(app: &Rc<App>) {
    if APERTO.with(|c| c.replace(true)) {
        return; // già aperta
    }

    let base = Rc::new(RefCell::new(app.config()));

    let dialogo = gtk::Dialog::with_buttons(
        Some("Configurazione — Zirkio Terminal"),
        Some(&app.window),
        gtk::DialogFlags::empty(),
        &[
            ("Predefiniti", gtk::ResponseType::Other(1)),
            ("Chiudi", gtk::ResponseType::Close),
            ("Salva", gtk::ResponseType::Accept),
        ],
    );
    dialogo.set_modal(false);
    dialogo.set_default_size(620, 520);

    let taccuino = gtk::Notebook::new();
    dialogo.content_area().pack_start(&taccuino, true, true, 0);

    // ------------------------------------------------------------ Aspetto
    let aspetto = gtk::Grid::new();
    aspetto.set_row_spacing(8);
    aspetto.set_column_spacing(12);
    aspetto.set_border_width(16);
    aspetto.set_column_homogeneous(false);

    let mut r = 0;
    aspetto.attach(&sezione("Carattere"), 0, r, 2, 1);
    r += 1;

    let carattere = gtk::ComboBoxText::new();
    let famiglie = famiglie_mono();
    carattere.append(Some(""), "monospace (quello di sistema)");
    for f in &famiglie {
        carattere.append(Some(f), f);
    }
    let attuale = base.borrow().font.family.clone().unwrap_or_default();
    if attuale.is_empty() {
        carattere.set_active_id(Some(""));
    } else if !carattere.set_active_id(Some(&attuale)) {
        carattere.append(Some(&attuale), &format!("{attuale} (non installato)"));
        carattere.set_active_id(Some(&attuale));
    }
    riga(&aspetto, r, "Carattere", &carattere);
    r += 1;

    let dimensione = gtk::SpinButton::with_range(6.0, 48.0, 1.0);
    dimensione.set_digits(1);
    dimensione.set_value(base.borrow().font.size as f64);
    dimensione.set_tooltip_text(Some("Dimensione del carattere in punti"));
    riga(&aspetto, r, "Dimensione", &dimensione);
    r += 1;

    let interlinea = gtk::SpinButton::with_range(1.0, 2.2, 0.02);
    interlinea.set_digits(2);
    interlinea.set_value(base.borrow().font.line_height as f64);
    interlinea.set_tooltip_text(Some("Spazio fra le righe (1,00 = attaccate)"));
    riga(&aspetto, r, "Interlinea", &interlinea);
    r += 1;

    let neretto = gtk::SpinButton::with_range(0.0, 1.0, 0.05);
    neretto.set_digits(2);
    neretto.set_value(base.borrow().font.bold_strength as f64);
    neretto.set_tooltip_text(Some("Quanto inspessire il grassetto quando il carattere non ha il neretto"));
    riga(&aspetto, r, "Grassetto finto", &neretto);
    r += 1;

    aspetto.attach(&sezione("Aspetto"), 0, r, 2, 1);
    r += 1;

    let tema = gtk::ComboBoxText::new();
    tema.append(Some("dark"), "Zirkio Dark");
    tema.append(Some("light"), "Zirkio Light");
    let chiaro = base.borrow().theme.name.to_ascii_lowercase().contains("light");
    tema.set_active_id(Some(if chiaro { "light" } else { "dark" }));
    riga(&aspetto, r, "Tema del terminale", &tema);
    r += 1;

    let margine = gtk::SpinButton::with_range(0.0, 48.0, 1.0);
    margine.set_digits(0);
    margine.set_value(base.borrow().padding as f64);
    riga(&aspetto, r, "Margine interno", &margine);
    r += 1;

    taccuino.append_page(&aspetto, Some(&gtk::Label::new(Some("Aspetto"))));

    // ---------------------------------------------------------- Terminale
    let term = gtk::Grid::new();
    term.set_row_spacing(8);
    term.set_column_spacing(12);
    term.set_border_width(16);

    let mut r = 0;
    term.attach(&sezione("Comportamento"), 0, r, 2, 1);
    r += 1;

    let cronologia = gtk::SpinButton::with_range(1000.0, 500_000.0, 1000.0);
    cronologia.set_digits(0);
    cronologia.set_value(base.borrow().scrollback as f64);
    cronologia.set_tooltip_text(Some("Righe tenute in memoria all'indietro (più righe = più RAM)"));
    riga(&term, r, "Righe di cronologia", &cronologia);
    r += 1;

    let cursore = gtk::ComboBoxText::new();
    cursore.append(Some("block"), "Blocco");
    cursore.append(Some("beam"), "Barra verticale");
    cursore.append(Some("underline"), "Sottolineato");
    cursore.set_active_id(Some("block"));
    riga(&term, r, "Cursore", &cursore);
    r += 1;

    let opacita = gtk::SpinButton::with_range(0.5, 1.0, 0.05);
    opacita.set_digits(2);
    opacita.set_value(base.borrow().window.opacity as f64);
    riga(&term, r, "Opacità finestra", &opacita);
    r += 1;

    term.attach(&sezione("Avvio"), 0, r, 2, 1);
    r += 1;

    let shell = gtk::Entry::new();
    if let Some(s) = &base.borrow().shell {
        shell.set_text(s);
    }
    shell.set_placeholder_text(Some("vuoto = la tua shell di login"));
    riga(&term, r, "Programma da avviare", &shell);
    r += 1;

    term.attach(&sezione("Sudo"), 0, r, 2, 1);
    r += 1;

    let sudo_ins = gtk::CheckButton::with_label("Inserisci la password quando sudo la chiede");
    sudo_ins.set_active(base.borrow().sudo.inserimento);
    sudo_ins.set_tooltip_text(Some(
        "La password si scrive solo se davanti al terminale c'è davvero sudo \
         (controllo sul processo in primo piano) e l'eco del terminale è spento",
    ));
    term.attach(&sudo_ins, 0, r, 2, 1);
    r += 1;

    let sudo_ric = gtk::CheckButton::with_label("Salva le password nel portachiavi di sistema");
    sudo_ric.set_active(base.borrow().sudo.ricorda);
    sudo_ric.set_sensitive(crate::sudo::portachiavi_disponibile());
    sudo_ric.set_tooltip_text(Some(if crate::sudo::portachiavi_disponibile() {
        "Una password per macchina: quelle che digiti restano nel portachiavi e non \
         si richiedono più. Senza questa casella valgono solo finché l'app è aperta"
    } else {
        "secret-tool non è installato (pacchetto libsecret): la password può stare solo in memoria"
    }));
    term.attach(&sudo_ric, 0, r, 2, 1);
    r += 1;

    let riga_password = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let pulsante_password = gtk::Button::with_label("Imposta la password…");
    let stato_password = gtk::Label::new(None);
    stato_password.set_xalign(0.0);
    riga_password.pack_start(&pulsante_password, false, false, 0);
    riga_password.pack_start(&stato_password, false, false, 0);
    term.attach(&riga_password, 0, r, 2, 1);
    r += 1;

    taccuino.append_page(&term, Some(&gtk::Label::new(Some("Terminale"))));

    // --------------------------------------------------------- Informazioni
    let info = gtk::Box::new(gtk::Orientation::Vertical, 6);
    info.set_border_width(16);
    let l_info = gtk::Label::new(None);
    l_info.set_xalign(0.0);
    l_info.set_line_wrap(true);
    let mut d = app.diagnostica();
    d.push('\n');
    d.push_str("configurazione: ");
    d.push_str(
        &Config::path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(nessun percorso)".into()),
    );
    l_info.set_text(&d);
    info.pack_start(&l_info, false, false, 0);
    let suggerimento = gtk::Label::new(None);
    suggerimento.set_xalign(0.0);
    suggerimento.set_markup(
        "<span size=\"small\" alpha=\"60%\">I caratteri elencati sono quelli installati sul sistema. \
         Per averne altri: metti i .ttf in ~/.local/share/fonts e lancia «fc-cache -f», poi riapri questa finestra.</span>",
    );
    suggerimento.set_line_wrap(true);
    info.pack_start(&suggerimento, false, false, 0);
    taccuino.append_page(&info, Some(&gtk::Label::new(Some("Informazioni"))));

    // Stato della password: si aggiorna all'apertura e dopo «Imposta la password…».
    let aggiorna_password: Rc<dyn Fn()> = {
        let stato = stato_password.clone();
        Rc::new(move || {
            let testo = if crate::sudo::c_e_password(crate::sudo::CHIAVE_LOCALE) {
                "impostata per questo computer"
            } else {
                "nessuna password impostata per questo computer"
            };
            stato.set_markup(&format!(
                "<span size=\"small\" alpha=\"70%\">{testo}</span>"
            ));
        })
    };
    aggiorna_password();
    {
        let finestra = dialogo.clone();
        let aggiorna = aggiorna_password.clone();
        let ricorda = sudo_ric.clone();
        pulsante_password.connect_clicked(move |_| {
            crate::sudo::dialogo_imposta(&finestra, aggiorna.clone(), ricorda.is_active());
        });
    }

    // ------------------------------------------------------------- applica
    let leggi = {
        let base = base.clone();
        let carattere = carattere.clone();
        let dimensione = dimensione.clone();
        let interlinea = interlinea.clone();
        let neretto = neretto.clone();
        let tema = tema.clone();
        let margine = margine.clone();
        let cronologia = cronologia.clone();
        let cursore = cursore.clone();
        let opacita = opacita.clone();
        let shell = shell.clone();
        let sudo_ins = sudo_ins.clone();
        let sudo_ric = sudo_ric.clone();
        move || -> Config {
            let mut c = base.borrow().clone();
            let fam = carattere.active_id().map(|s| s.to_string()).unwrap_or_default();
            c.font.family = if fam.is_empty() { None } else { Some(fam) };
            c.font.size = dimensione.value() as f32;
            c.font.line_height = interlinea.value() as f32;
            c.font.bold_strength = neretto.value() as f32;
            c.padding = margine.value() as u32;
            c.scrollback = cronologia.value() as usize;
            c.window.opacity = opacita.value() as f32;
            match tema.active_id().as_deref() {
                Some("light") => c.theme = Theme::light(),
                _ => c.theme = Theme::dark(),
            }
            let s = shell.text().to_string();
            c.shell = if s.trim().is_empty() { None } else { Some(s) };
            c.sudo.inserimento = sudo_ins.is_active();
            c.sudo.ricorda = sudo_ric.is_active();
            c
        }
    };

    let collega = {
        let app = app.clone();
        let base = base.clone();
        let leggi = leggi.clone();
        move || {
            let nuovo = leggi();
            *base.borrow_mut() = nuovo.clone();
            app.apply_config(nuovo);
        }
    };

    macro_rules! vivo {
        ($w:expr) => {{
            let c = collega.clone();
            $w.connect_changed(move |_| c());
        }};
    }

    let c = collega.clone();
    carattere.connect_changed(move |_| c());
    let c = collega.clone();
    dimensione.connect_value_changed(move |_| c());
    let c = collega.clone();
    interlinea.connect_value_changed(move |_| c());
    let c = collega.clone();
    neretto.connect_value_changed(move |_| c());
    let c = collega.clone();
    tema.connect_changed(move |_| c());
    let c = collega.clone();
    margine.connect_value_changed(move |_| c());
    let c = collega.clone();
    cronologia.connect_value_changed(move |_| c());
    let c = collega.clone();
    cursore.connect_changed(move |_| c());
    let c = collega.clone();
    opacita.connect_value_changed(move |_| c());
    let c = collega.clone();
    shell.connect_changed(move |_| c());
    let c = collega.clone();
    sudo_ins.connect_toggled(move |_| c());
    let c = collega.clone();
    sudo_ric.connect_toggled(move |_| c());

    // ------------------------------------------------------------ risposte
    {
        let app = app.clone();
        let leggi = leggi.clone();
        let dialogo2 = dialogo.clone();
        let dimensione = dimensione.clone();
        let interlinea = interlinea.clone();
        let neretto = neretto.clone();
        let margine = margine.clone();
        let cronologia = cronologia.clone();
        let opacita = opacita.clone();
        let carattere = carattere.clone();
        let tema = tema.clone();
        let shell = shell.clone();
        let cursore = cursore.clone();
        let sudo_ins = sudo_ins.clone();
        let sudo_ric = sudo_ric.clone();
        dialogo.connect_response(move |_, risposta| match risposta {
            gtk::ResponseType::Accept => {
                // «Salva» salva **e chiude**: la finestra resta aperta solo se il
                // salvataggio è fallito (e in quel caso l'errore si vede).
                let c = leggi();
                app.apply_config(c.clone());
                match c.save() {
                    Ok(p) => {
                        eprintln!("[prefs] configurazione salvata in {}", p.display());
                        dialogo2.close();
                    }
                    Err(e) => {
                        eprintln!("[prefs] non ho potuto salvare: {e}");
                        let d = gtk::MessageDialog::new(
                            Some(&dialogo2),
                            gtk::DialogFlags::MODAL,
                            gtk::MessageType::Error,
                            gtk::ButtonsType::Close,
                            &format!("Non ho potuto salvare la configurazione:\n{e}"),
                        );
                        d.connect_response(|d, _| d.close());
                        d.show_all();
                    }
                }
            }
            gtk::ResponseType::Other(1) => {
                let d = Config::default();
                dimensione.set_value(d.font.size as f64);
                interlinea.set_value(d.font.line_height as f64);
                neretto.set_value(d.font.bold_strength as f64);
                margine.set_value(d.padding as f64);
                cronologia.set_value(d.scrollback as f64);
                opacita.set_value(d.window.opacity as f64);
                let fam = d.font.family.clone().unwrap_or_default();
                if !carattere.set_active_id(Some(&fam)) {
                    carattere.set_active_id(Some(""));
                }
                tema.set_active_id(Some("dark"));
                cursore.set_active_id(Some("block"));
                shell.set_text("");
                sudo_ins.set_active(false);
                sudo_ric.set_active(false);
                app.apply_config(d);
            }
            _ => {
                dialogo2.close();
            }
        });
    }

    dialogo.connect_destroy(move |_| APERTO.with(|c| c.set(false)));
    dialogo.show_all();
}
