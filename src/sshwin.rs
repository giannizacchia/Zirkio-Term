//! Finestra **Sessioni SSH**: elenco a sinistra, dati della sessione a destra.
//!
//! I campi sono: nome, descrizione, host, utente, porta, chiave SSH (facoltativa),
//! comando da eseguire sul remoto (facoltativo) e il toggle «invia la chiave di questo
//! computer», che alla prima connessione installa la chiave pubblica sul remoto
//! (`ssh-copy-id`, la password si digita una volta) per entrare poi senza password.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;

use crate::app::App;
use crate::ssh::{self, Sessione};

thread_local! {
    static APERTO: Cell<bool> = const { Cell::new(false) };
}

fn campo(griglia: &gtk::Grid, r: i32, testo: &str, controllo: &impl IsA<gtk::Widget>) {
    let l = gtk::Label::new(Some(testo));
    l.set_xalign(1.0);
    l.set_margin_end(12);
    griglia.attach(&l, 0, r, 1, 1);
    griglia.attach(controllo, 1, r, 1, 1);
}

pub fn apri(app: &Rc<App>) {
    if APERTO.with(|c| c.replace(true)) {
        return; // già aperta
    }

    let sessioni: Rc<RefCell<Vec<Sessione>>> = Rc::new(RefCell::new(app.sessioni()));
    // -1 = si sta scrivendo una sessione nuova
    let indice = Rc::new(Cell::new(-1i32));

    let dialogo = gtk::Dialog::with_buttons(
        Some("Sessioni SSH — Zirkio Terminal"),
        Some(&app.window),
        gtk::DialogFlags::empty(),
        &[
            ("Elimina", gtk::ResponseType::Other(1)),
            ("Nuova", gtk::ResponseType::Other(2)),
            ("Salva", gtk::ResponseType::Other(3)),
            ("Connetti", gtk::ResponseType::Other(4)),
            ("Chiudi", gtk::ResponseType::Close),
        ],
    );
    dialogo.set_modal(false);
    dialogo.set_default_size(820, 520);

    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_position(260);
    dialogo.content_area().pack_start(&paned, true, true, 0);

    // ------------------------------------------------------------- elenco
    let elenco = gtk::ListBox::new();
    elenco.set_selection_mode(gtk::SelectionMode::Single);
    let scorr = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scorr.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scorr.add(&elenco);
    paned.pack1(&scorr, true, false);

    // ------------------------------------------------------------- modulo
    let griglia = gtk::Grid::new();
    griglia.set_row_spacing(8);
    griglia.set_column_spacing(10);
    griglia.set_border_width(16);
    griglia.set_hexpand(true);
    griglia.set_valign(gtk::Align::Start);

    let nome = gtk::Entry::new();
    nome.set_placeholder_text(Some("es. jan, server, raspberry"));
    campo(&griglia, 0, "Nome", &nome);

    let descrizione = gtk::Entry::new();
    descrizione.set_placeholder_text(Some("a cosa serve questa macchina"));
    campo(&griglia, 1, "Descrizione", &descrizione);

    let host = gtk::Entry::new();
    host.set_placeholder_text(Some("192.0.2.10 oppure nome.dominio"));
    campo(&griglia, 2, "Host", &host);

    let utente = gtk::Entry::new();
    utente.set_text(&ssh::utente_locale());
    campo(&griglia, 3, "Utente", &utente);

    let porta = gtk::SpinButton::with_range(1.0, 65535.0, 1.0);
    porta.set_value(22.0);
    campo(&griglia, 4, "Porta", &porta);

    let riga_chiave = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let chiave = gtk::Entry::new();
    chiave.set_placeholder_text(Some("~/.ssh/id_ed25519 (vuoto = quella predefinita)"));
    chiave.set_hexpand(true);
    let sfoglia = gtk::Button::with_label("Scegli...");
    riga_chiave.pack_start(&chiave, true, true, 0);
    riga_chiave.pack_end(&sfoglia, false, false, 0);
    campo(&griglia, 5, "Chiave SSH", &riga_chiave);

    let remoto = gtk::Entry::new();
    remoto.set_placeholder_text(Some("facoltativo: comando da eseguire appena entrato"));
    campo(&griglia, 6, "Comando remoto", &remoto);

    let invia = gtk::CheckButton::with_label("Invia la chiave di questo computer (login senza password)");
    invia.set_tooltip_text(Some(
        "Alla prossima connessione installa la chiave pubblica sul remoto con ssh-copy-id: \
         chiede la password una volta sola, poi si entra senza.",
    ));
    griglia.attach(&invia, 1, 7, 1, 1);

    let stato = gtk::Label::new(None);
    stato.set_xalign(0.0);
    stato.set_line_wrap(true);
    griglia.attach(&stato, 0, 8, 2, 1);

    paned.pack2(&griglia, true, false);

    // ------------------------------------------------------------- funzioni
    let scrivi_modulo = {
        let nome = nome.clone();
        let descrizione = descrizione.clone();
        let host = host.clone();
        let utente = utente.clone();
        let porta = porta.clone();
        let chiave = chiave.clone();
        let remoto = remoto.clone();
        let invia = invia.clone();
        let stato = stato.clone();
        let indice = indice.clone();
        Rc::new(move |s: Option<&Sessione>| {
            match s {
                Some(s) => {
                    nome.set_text(&s.nome);
                    descrizione.set_text(&s.descrizione);
                    host.set_text(&s.host);
                    utente.set_text(&s.utente);
                    porta.set_value(s.porta as f64);
                    chiave.set_text(s.chiave.as_deref().unwrap_or(""));
                    remoto.set_text(s.comando_remoto.as_deref().unwrap_or(""));
                    invia.set_active(s.invia_chiave);
                }
                None => {
                    nome.set_text("");
                    descrizione.set_text("");
                    host.set_text("");
                    utente.set_text(&ssh::utente_locale());
                    porta.set_value(22.0);
                    chiave.set_text("");
                    remoto.set_text("");
                    invia.set_active(false);
                    indice.set(-1);
                }
            }
            stato.set_text("");
        })
    };

    let leggi_modulo = {
        let nome = nome.clone();
        let descrizione = descrizione.clone();
        let host = host.clone();
        let utente = utente.clone();
        let porta = porta.clone();
        let chiave = chiave.clone();
        let remoto = remoto.clone();
        let invia = invia.clone();
        Rc::new(move || Sessione {
            nome: nome.text().trim().to_string(),
            descrizione: descrizione.text().trim().to_string(),
            host: host.text().trim().to_string(),
            utente: utente.text().trim().to_string(),
            porta: porta.value_as_int().clamp(1, 65535) as u16,
            chiave: Some(chiave.text().trim().to_string()).filter(|k| !k.is_empty()),
            comando_remoto: Some(remoto.text().trim().to_string()).filter(|c| !c.is_empty()),
            invia_chiave: invia.is_active(),
        })
    };

    // riempie l'elenco e seleziona una riga
    let riempi_elenco: Rc<dyn Fn(Option<usize>)> = {
        let elenco = elenco.clone();
        let sessioni = sessioni.clone();
        let scrivi_modulo = scrivi_modulo.clone();
        let indice = indice.clone();
        Rc::new(move |seleziona: Option<usize>| {
            for riga in elenco.children() {
                elenco.remove(&riga);
            }
            let v = sessioni.borrow();
            for (i, s) in v.iter().enumerate() {
                let riga = gtk::ListBoxRow::new();
                let box_riga = gtk::Box::new(gtk::Orientation::Vertical, 2);
                box_riga.set_border_width(6);
                let titolo = gtk::Label::new(None);
                titolo.set_markup(&format!("<b>{}</b>", gtk::glib::markup_escape_text(&s.nome)));
                titolo.set_xalign(0.0);
                let sotto = gtk::Label::new(None);
                sotto.set_markup(&format!(
                    "<span size=\"small\" alpha=\"60%\">{}</span>",
                    gtk::glib::markup_escape_text(&if s.descrizione.is_empty() {
                        s.riassunto()
                    } else {
                        format!("{} · {}", s.descrizione, s.riassunto())
                    })
                ));
                sotto.set_xalign(0.0);
                box_riga.pack_start(&titolo, false, false, 0);
                box_riga.pack_start(&sotto, false, false, 0);
                riga.add(&box_riga);
                riga.set_widget_name(&i.to_string());
                elenco.add(&riga);
                // ⚠️ La riga nuova va mostrata ESPLICITAMENTE: aggiunta a un contenitore
                // già visibile, GTK non la mostra da sola (la sessione appena salvata
                // «non compariva» finché non si riapriva la finestra).
                riga.show_all();
            }
            elenco.show_all();
            if let Some(i) = seleziona {
                if let Some(riga) = elenco.row_at_index(i as i32) {
                    elenco.select_row(Some(&riga));
                }
                indice.set(i as i32);
                scrivi_modulo(v.get(i));
            }
        })
    };

    // selezione dall'elenco -> modulo
    {
        let sessioni = sessioni.clone();
        let scrivi_modulo = scrivi_modulo.clone();
        let indice = indice.clone();
        let stato = stato.clone();
        elenco.connect_row_selected(move |_, riga| {
            let Some(riga) = riga else { return };
            let i = riga.index();
            if i < 0 {
                return;
            }
            indice.set(i);
            let s = sessioni.borrow().get(i as usize).cloned();
            scrivi_modulo(s.as_ref());
            stato.set_text("");
        });
    }

    // «Scegli...» per il file della chiave
    {
        let chiave = chiave.clone();
        let dialogo2 = dialogo.clone();
        sfoglia.connect_clicked(move |_| {
            let scelta = gtk::FileChooserDialog::with_buttons(
                Some("Scegli la chiave privata"),
                Some(&dialogo2),
                gtk::FileChooserAction::Open,
                &[
                    ("Annulla", gtk::ResponseType::Cancel),
                    ("Apri", gtk::ResponseType::Accept),
                ],
            );
            scelta.set_filename(&format!("{}/.ssh", std::env::var("HOME").unwrap_or_default()));
            if scelta.run() == gtk::ResponseType::Accept {
                if let Some(percorso) = scelta.filename() {
                    let p = percorso.to_string_lossy().to_string();
                    // Se si sceglie il file .pub si tiene la chiave privata.
                    chiave.set_text(p.strip_suffix(".pub").unwrap_or(&p));
                }
            }
            scelta.close();
        });
    }

    // salva la sessione del modulo (nuova o modificata).  Restituisce il nome.
    let salva_modulo = {
        let sessioni = sessioni.clone();
        let leggi_modulo = leggi_modulo.clone();
        let indice = indice.clone();
        let stato = stato.clone();
        let app = app.clone();
        let riempi_elenco = riempi_elenco.clone();
        Rc::new(move || -> Option<String> {
            let s = leggi_modulo();
            if let Err(e) = s.valida() {
                stato.set_markup(&format!("<span foreground=\"#c01c28\">{}</span>", gtk::glib::markup_escape_text(&e)));
                return None;
            }
            {
                let mut v = sessioni.borrow_mut();
                let i = indice.get();
                if i >= 0 && (i as usize) < v.len() {
                    v[i as usize] = s.clone();
                } else {
                    // nome già usato? si aggiorna quella esistente
                    match v.iter().position(|x| x.nome == s.nome) {
                        Some(pos) => v[pos] = s.clone(),
                        None => v.push(s.clone()),
                    }
                }
            }
            if let Err(e) = app.salva_sessioni(&sessioni.borrow()) {
                stato.set_markup(&format!("<span foreground=\"#c01c28\">{}</span>", gtk::glib::markup_escape_text(&e)));
                return None;
            }
            let pos = sessioni.borrow().iter().position(|x| x.nome == s.nome);
            riempi_elenco(pos);
            stato.set_text(&format!("sessione «{}» salvata", s.nome));
            Some(s.nome)
        })
    };

    {
        let riempi_elenco = riempi_elenco.clone();
        riempi_elenco(if sessioni.borrow().is_empty() { None } else { Some(0) });
    }

    // ------------------------------------------------------------- risposte
    {
        let app = app.clone();
        let salva_modulo = salva_modulo.clone();
        let riempi_elenco = riempi_elenco.clone();
        let sessioni = sessioni.clone();
        let indice = indice.clone();
        let scrivi_modulo = scrivi_modulo.clone();
        let stato = stato.clone();
        let dialogo2 = dialogo.clone();
        dialogo.connect_response(move |_, risposta| match risposta {
            gtk::ResponseType::Other(1) => {
                // Elimina
                let i = indice.get();
                if i < 0 || i as usize >= sessioni.borrow().len() {
                    stato.set_text("scegli prima una sessione dall'elenco");
                    return;
                }
                let nome = sessioni.borrow()[i as usize].nome.clone();
                sessioni.borrow_mut().remove(i as usize);
                if let Err(e) = app.salva_sessioni(&sessioni.borrow()) {
                    stato.set_markup(&format!(
                        "<span foreground=\"#c01c28\">{}</span>",
                        gtk::glib::markup_escape_text(&e)
                    ));
                    return;
                }
                indice.set(-1);
                scrivi_modulo(None);
                riempi_elenco(if sessioni.borrow().is_empty() { None } else { Some(0) });
                stato.set_text(&format!("sessione «{nome}» eliminata"));
            }
            gtk::ResponseType::Other(2) => {
                // Nuova
                indice.set(-1);
                scrivi_modulo(None);
                if let Some(riga) = elenco.row_at_index(-1) {
                    elenco.select_row(Some(&riga));
                }
                elenco.unselect_all();
                stato.set_text("compila i dati e premi «Salva»");
            }
            gtk::ResponseType::Other(3) => {
                salva_modulo();
            }
            gtk::ResponseType::Other(4) => {
                // Connetti
                let Some(nome) = salva_modulo() else { return };
                match app.apri_sessione(&nome) {
                    Ok(()) => dialogo2.close(),
                    Err(e) => stato.set_markup(&format!(
                        "<span foreground=\"#c01c28\">{}</span>",
                        gtk::glib::markup_escape_text(&e)
                    )),
                }
            }
            _ => {
                dialogo2.close();
            }
        });
    }

    dialogo.connect_destroy(move |_| APERTO.with(|c| c.set(false)));
    dialogo.show_all();
}
