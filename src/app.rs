//! Applicazione GTK3: finestra, HeaderBar, schede, divisioni, menu, scorciatoie.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use gtk::prelude::*;

use crate::config::{Config, Theme};
use crate::render::font::FontSet;
use crate::view::View;

/// Una voce di menu: un comando, oppure un sottomenu con dentro altre voci.
#[derive(Clone)]
pub enum Voce {
    Comando(&'static str, &'static str, Action),
    Sottomenu(&'static str, Vec<Voce>),
    /// Sessione SSH salvata: nome e descrizione (il clic si connette).
    Sessione(String, String),
    /// Riga di separazione dentro un sottomenu.
    Separatore,
    /// Voce informativa, non cliccabile (es. «nessuna connessione salvata»).
    Inerte(String),
}

/// Comandi dell'applicazione (menu, scorciatoie, pulsanti).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    NewTab,
    ClosePane,
    SplitRight,
    SplitDown,
    Copy,
    Paste,
    SelectAll,
    FontBigger,
    FontSmaller,
    FontReset,
    ThemeNext,
    NextTab,
    PrevTab,
    FocusNext,
    FocusPrev,
    ScrollPageUp,
    ScrollPageDown,
    ScrollTop,
    ScrollBottom,
    Help,
    Prefs,
    /// Finestra «Sessioni SSH» (elenco e dati delle connessioni salvate).
    SessioniSsh,
    /// Finestra «Trasferimenti» (due riquadri + coda di `scp`).
    Trasferimenti,
    Quit,
}

struct TabInfo {
    /// Il widget che sta dentro la pagina del notebook (un pannello o un GtkPaned).
    #[allow(dead_code)]
    root: gtk::Widget,
    views: Vec<u64>,
}

pub struct App {
    pub window: gtk::ApplicationWindow,
    pub header: gtk::HeaderBar,
    pub notebook: gtk::Notebook,
    cfg: RefCell<Config>,
    fonts: RefCell<Rc<FontSet>>,
    views: RefCell<HashMap<u64, Rc<View>>>,
    tabs: RefCell<Vec<TabInfo>>,
    active: Cell<u64>,
    next_id: Cell<u64>,
    title_shown: RefCell<HashMap<u64, String>>,
    me: RefCell<Weak<App>>,
    /// Menu del tasto destro a schermo: va tenuto vivo finché è aperto.
    menu_aperto: RefCell<Option<gtk::Menu>>,
    /// Sessioni SSH salvate (`~/.config/zirkio-term/ssh.toml`).
    sessioni: RefCell<Vec<crate::ssh::Sessione>>,
    /// Il pulsante ⋮, per rifare il menu quando cambiano le sessioni.
    menu_bottone: RefCell<Option<gtk::MenuButton>>,
    /// Il pulsante **SSH** dell'HeaderBar (menu con le connessioni salvate).
    menu_ssh_bottone: RefCell<Option<gtk::MenuButton>>,
}

impl App {
    pub fn new(gtk_app: &gtk::Application, cfg: Config) -> Result<Rc<App>, String> {
        let fonts = Rc::new(FontSet::load(
            cfg.font.family.as_deref(),
            cfg.font.size,
            cfg.font.line_height,
            cfg.font.bold_strength,
        )?);

        let window = gtk::ApplicationWindow::new(gtk_app);
        window.set_title("zirkio-term");
        window.set_default_size(cfg.window.width as i32, cfg.window.height as i32);

        let header = gtk::HeaderBar::new();
        header.set_show_close_button(true);
        header.set_title(Some("zirkio-term"));
        header.set_subtitle(Some("terminale"));
        window.set_titlebar(Some(&header));

        let notebook = gtk::Notebook::new();
        notebook.set_scrollable(true);
        notebook.set_show_border(false);
        window.add(&notebook);

        let app = Rc::new(App {
            window,
            header,
            notebook,
            cfg: RefCell::new(cfg),
            fonts: RefCell::new(fonts),
            views: RefCell::new(HashMap::new()),
            tabs: RefCell::new(Vec::new()),
            active: Cell::new(0),
            next_id: Cell::new(1),
            title_shown: RefCell::new(HashMap::new()),
            me: RefCell::new(Weak::new()),
            menu_aperto: RefCell::new(None),
            sessioni: RefCell::new(crate::ssh::carica()),
            menu_bottone: RefCell::new(None),
            menu_ssh_bottone: RefCell::new(None),
        });
        *app.me.borrow_mut() = Rc::downgrade(&app);

        let dbg = std::env::var_os("ZIRKIO_STATS").is_some();
        if dbg { eprintln!("[stats] app: costruisco le barre"); }
        app.build_header();
        installa_stile_menu();
        app.build_menu();
        app.build_menu_ssh();
        app.build_bottone_trasferimenti();
        app.connect_events();
        if dbg { eprintln!("[stats] app: prima scheda"); }
        app.new_tab()?;
        if dbg { eprintln!("[stats] app: show_all"); }
        app.window.show_all();
        // Autotest senza tastiera: ZIRKIO_SELFTEST="split-right,theme,tab"
        if let Ok(steps) = std::env::var("ZIRKIO_SELFTEST") {
            let me = app.me.borrow().clone();
            glib::timeout_add_local(std::time::Duration::from_millis(2500), move || {
                if let Some(a) = me.upgrade() {
                    a.selftest(&steps);
                }
                glib::ControlFlow::Break
            });
        }
        if dbg {
            let w = app.window.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(700), move || {
                eprintln!(
                    "[stats] finestra: visibile={} mappata={} misura={}x{} gdk_window={}",
                    w.is_visible(),
                    w.is_mapped(),
                    w.allocated_width(),
                    w.allocated_height(),
                    w.window().is_some()
                );
                glib::ControlFlow::Break
            });
            eprintln!("[stats] app: pronto");
        }
        Ok(app)
    }

    // ------------------------------------------------------------ costruzione

    /// Tema delle icone, se c'è: senza schermo non si deve morire (capita in
    /// ambienti senza display, dove comunque non si arriva a costruire la finestra).
    fn icon_theme(&self) -> Option<gtk::IconTheme> {
        let screen = gdk::Screen::default()?;
        gtk::IconTheme::for_screen(&screen)
    }

    fn icon_button(&self, icons: &[&str], label: &str, tooltip: &str) -> gtk::Button {
        let theme = self.icon_theme();
        let mut btn: Option<gtk::Button> = None;
        for name in icons {
            if theme.as_ref().is_some_and(|t| t.has_icon(name)) {
                btn = Some(gtk::Button::from_icon_name(Some(name), gtk::IconSize::Button));
                break;
            }
        }
        let btn = btn.unwrap_or_else(|| gtk::Button::with_label(label));
        btn.set_tooltip_text(Some(tooltip));
        btn.set_valign(gtk::Align::Center);
        btn
    }

    fn build_header(&self) {
        let me = self.me.borrow().clone();
        let btn_new = self.icon_button(&["tab-new-symbolic", "list-add-symbolic"], "+", "Nuova scheda (Ctrl+Maiusc+T)");
        let me_new = me.clone();
        btn_new.connect_clicked(move |_| {
            if let Some(app) = me_new.upgrade() {
                let _ = app.new_tab();
            }
        });
        self.header.pack_start(&btn_new);

        // Pulsante "dividi": due voci in un menu.
        let split_menu = gtk::Menu::new();
        for (label, orient) in [
            ("Dividi a destra", gtk::Orientation::Horizontal),
            ("Dividi in basso", gtk::Orientation::Vertical),
        ] {
            let item = gtk::MenuItem::with_label(label);
            let me_item = me.clone();
            item.connect_activate(move |_| {
                if let Some(app) = me_item.upgrade() {
                    app.split(orient);
                }
            });
            split_menu.append(&item);
        }
        split_menu.show_all();
        let btn_split = gtk::MenuButton::new();
        let theme = self.icon_theme();
        let split_icon = ["view-dual-symbolic", "view-split-left-right-symbolic", "window-new-symbolic"]
            .iter()
            .find(|n| theme.as_ref().is_some_and(|t| t.has_icon(n)))
            .copied();
        match split_icon {
            Some(name) => btn_split.set_image(Some(&gtk::Image::from_icon_name(Some(name), gtk::IconSize::Button))),
            None => btn_split.set_label("||"),
        }
        btn_split.set_tooltip_text(Some("Dividi il pannello"));
        btn_split.set_popup(Some(&split_menu));
        btn_split.set_valign(gtk::Align::Center);
        self.header.pack_start(&btn_split);
    }

    /// Menu completo delle azioni: un **`GtkMenu` standard** dentro un `MenuButton`.
    ///
    /// Prima era un popover riempito di pulsanti (uno per voce), costruito a mano: un
    /// ripiego nato il 24/10 perché i clic sulle voci non arrivavano.  Ma un menu fatto
    /// in casa non ha il comportamento nativo dei menu (grab, tastiera, chiusura,
    /// posizionamento su Wayland) ed è la ragione dei difetti del 24/10 sera: il popover
    /// del ⋮ veniva chiuso subito dal compositore.  Ora si usa il menu di GTK.
    /// Menu completo delle azioni: **`GtkMenu` standard** dentro un `MenuButton`.
    ///
    /// Le voci sono raggruppate per tipo, con un separatore fra un gruppo e l'altro, e
    /// l'aspetto è quello del menu di sistema (etichette allineate in colonna,
    /// scorciatoie a destra, sottomenu vero per l'aspetto).
    pub fn action_menu(&self) -> gtk::Menu {
        self.menu_da_gruppi(
            vec![
                vec![Voce::Comando("Nuova scheda", "Ctrl+Maiusc+T", Action::NewTab)],
                vec![
                    Voce::Comando("Dividi a destra", "Ctrl+Maiusc+D", Action::SplitRight),
                    Voce::Comando("Dividi in basso", "Ctrl+Maiusc+E", Action::SplitDown),
                    Voce::Comando("Chiudi pannello attivo", "Ctrl+Maiusc+W", Action::ClosePane),
                ],
                vec![
                    Voce::Comando("Copia", "Ctrl+Maiusc+C", Action::Copy),
                    Voce::Comando("Incolla", "Ctrl+Maiusc+V", Action::Paste),
                    Voce::Comando("Seleziona tutto", "Ctrl+Maiusc+A", Action::SelectAll),
                ],
                {
                    let mut voci_ssh: Vec<Voce> = vec![
                        Voce::Comando("Trasferimenti file...", "Ctrl+Maiusc+F", Action::Trasferimenti),
                        Voce::Comando("Nuova sessione...", "Ctrl+Maiusc+S", Action::SessioniSsh),
                        Voce::Comando("Gestisci sessioni...", "", Action::SessioniSsh),
                    ];
                    let sessioni = self.sessioni.borrow();
                    if !sessioni.is_empty() {
                        voci_ssh.push(Voce::Separatore);
                        for s in sessioni.iter() {
                            voci_ssh.push(Voce::Sessione(s.nome.clone(), s.descrizione.clone()));
                        }
                    }
                    drop(sessioni);
                    vec![Voce::Sottomenu("SSH", voci_ssh)]
                },
                vec![
                    Voce::Sottomenu(
                        "Aspetto",
                        vec![
                            Voce::Comando("Carattere più grande", "Ctrl+Maiusc++", Action::FontBigger),
                            Voce::Comando("Carattere più piccolo", "Ctrl+Maiusc+-", Action::FontSmaller),
                            Voce::Comando("Carattere predefinito", "Ctrl+Maiusc+0", Action::FontReset),
                            Voce::Comando("Cambia tema (chiaro/scuro)", "F2", Action::ThemeNext),
                        ],
                    ),
                    Voce::Comando("Configurazione...", "Ctrl+,", Action::Prefs),
                    Voce::Comando("Scorciatoie...", "F1", Action::Help),
                ],
                vec![Voce::Comando("Esci", "Ctrl+Maiusc+Q", Action::Quit)],
            ],
            None,
        )
    }

    /// Menu corto del tasto destro: si apre sul pannello sotto il cursore e la prima
    /// voce chiude **quel** pannello.
    pub fn pane_menu(&self, id: u64) -> gtk::Menu {
        self.menu_da_gruppi(
            vec![
                vec![
                    Voce::Comando("Chiudi questo pannello", "Ctrl+Maiusc+W", Action::ClosePane),
                    Voce::Comando("Dividi a destra", "Ctrl+Maiusc+D", Action::SplitRight),
                    Voce::Comando("Dividi in basso", "Ctrl+Maiusc+E", Action::SplitDown),
                ],
                vec![
                    Voce::Comando("Copia", "Ctrl+Maiusc+C", Action::Copy),
                    Voce::Comando("Incolla", "Ctrl+Maiusc+V", Action::Paste),
                ],
                vec![
                    Voce::Comando("Nuova scheda", "Ctrl+Maiusc+T", Action::NewTab),
                    Voce::Comando("Trasferimenti file...", "Ctrl+Maiusc+F", Action::Trasferimenti),
                    Voce::Comando("Configurazione...", "Ctrl+,", Action::Prefs),
                ],
            ],
            Some(id),
        )
    }

    /// Apre il menu del tasto destro sul pannello `id`, ancorato al puntatore.
    pub fn apri_menu_pannello(&self, id: u64, event: &gdk::EventButton) {
        let menu = self.pane_menu(id);
        // Tenuto vivo: un `GtkMenu` creato al volo e mostrato va conservato.
        *self.menu_aperto.borrow_mut() = Some(menu.clone());
        menu.popup_at_pointer(Some(event));
    }

    /// Costruisce un `GtkMenu` a gruppi, separati da una riga; l'etichetta più lunga
    /// del gruppo fissa la larghezza della colonna, così le scorciatoie restano
    /// allineate a destra e il menu è ordinato.
    fn menu_da_gruppi(&self, gruppi: Vec<Vec<Voce>>, target: Option<u64>) -> gtk::Menu {
        let menu = gtk::Menu::new();
        for (i, gruppo) in gruppi.into_iter().enumerate() {
            if i > 0 {
                menu.append(&separatore_menu());
            }
            for voce in gruppo {
                menu.append(&self.voce_menu(voce, target));
            }
        }
        // Il sottomenu "Aspetto" è già un `GtkMenu` completo: `show_all` in fondo.
        menu.show_all();
        menu
    }

    fn voce_menu(&self, voce: Voce, target: Option<u64>) -> gtk::MenuItem {
        match voce {
            Voce::Sottomenu(titolo, voci) => {
                let item = gtk::MenuItem::new();
                item.add(&riga_voce(titolo, ""));
                let sotto = gtk::Menu::new();
                for v in voci {
                    sotto.append(&self.voce_menu(v, target));
                }
                sotto.show_all();
                item.set_submenu(Some(&sotto));
                item
            }
            Voce::Separatore => separatore_menu(),
            Voce::Inerte(testo) => {
                let item = gtk::MenuItem::new();
                item.add(&riga_voce(&testo, ""));
                item.set_sensitive(false);
                item
            }
            Voce::Sessione(nome, descrizione) => {
                // Nome a sinistra e **descrizione accanto, in grigio e piccola**, con un
                // tetto di `MAX_CARATTERI_DESCRIZIONE` caratteri (oltre: «…», il testo
                // intero nel suggerimento).  Così il menu resta largo quanto il contenuto
                // invece di allargarsi con la descrizione più lunga.
                let item = gtk::MenuItem::new();
                item.add(&riga_sessione(&nome, &descrizione));
                item.set_tooltip_text(Some(&format!(
                    "connetti a {nome}{}",
                    if descrizione.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" — {descrizione}")
                    }
                )));
                let me = self.me.borrow().clone();
                item.connect_activate(move |_| {
                    if std::env::var_os("ZIRKIO_STATS").is_some() {
                        eprintln!("[stats] sessione ssh: {nome}");
                    }
                    if let Some(app) = me.upgrade() {
                        if let Err(e) = app.apri_sessione(&nome) {
                            app.error(&e);
                        }
                    }
                });
                item
            }
            Voce::Comando(label, accel, action) => {
                let item = gtk::MenuItem::new();
                item.add(&riga_voce(label, accel));
                let me = self.me.borrow().clone();
                let etichetta = label.to_string();
                item.connect_activate(move |_| {
                    if std::env::var_os("ZIRKIO_STATS").is_some() {
                        eprintln!("[stats] voce di menu: {etichetta}");
                    }
                    if let Some(app) = me.upgrade() {
                        match (action, target) {
                            (Action::ClosePane, Some(id)) => app.close_view(id),
                            _ => {
                                app.shortcut(action);
                            }
                        }
                    }
                });
                item
            }
        }
    }

    /// Menu **SSH** della barra: prima la voce per aprire l'interfaccia, poi una riga e
    /// sotto le connessioni memorizzate (una voce per sessione).
    pub fn menu_ssh(&self) -> gtk::Menu {
        let mut voci: Vec<Voce> = vec![
            Voce::Comando("Trasferimenti file...", "Ctrl+Maiusc+F", Action::Trasferimenti),
            Voce::Comando("Sessioni SSH...", "Ctrl+Maiusc+S", Action::SessioniSsh),
        ];
        {
            let sessioni = self.sessioni.borrow();
            voci.push(Voce::Separatore);
            if sessioni.is_empty() {
                voci.push(Voce::Inerte("(nessuna connessione salvata)".into()));
            } else {
                for s in sessioni.iter() {
                    voci.push(Voce::Sessione(s.nome.clone(), s.descrizione.clone()));
                }
            }
        }
        self.menu_da_gruppi(vec![voci], None)
    }

    fn build_menu_ssh(&self) {
        let btn = gtk::MenuButton::new();
        btn.set_label("SSH");
        btn.set_tooltip_text(Some("Sessioni SSH: connessioni salvate"));
        btn.set_valign(gtk::Align::Center);
        btn.set_popup(Some(&self.menu_ssh()));
        *self.menu_ssh_bottone.borrow_mut() = Some(btn.clone());
        // `pack_end` dopo il ⋮: nella barra il pulsante SSH finisce **a sinistra** del ⋮.
        self.header.pack_end(&btn);
    }

    /// Pulsante **⇅** della barra: apre la finestra dei trasferimenti (stessa finestra
    /// che si apre con `Ctrl+Maiusc+F` e con `@scp`).  Sta a sinistra del pulsante SSH.
    fn build_bottone_trasferimenti(&self) {
        let btn = self.icon_button(
            &[
                "mail-send-receive-symbolic",
                "network-transmit-receive-symbolic",
                "document-send-symbolic",
            ],
            "⇅",
            "Trasferimenti di file (Ctrl+Maiusc+F)",
        );
        let me = self.me.borrow().clone();
        btn.connect_clicked(move |_| {
            if let Some(app) = me.upgrade() {
                crate::scpwin::apri(&app);
            }
        });
        self.header.pack_end(&btn);
    }

    fn build_menu(&self) {
        // `MenuButton` + **`GtkMenu` vero** (`set_popup`, non `set_popover`): è la via
        // standard di GTK3 — grab, tastiera, chiusura e posizionamento li gestisce GTK
        // (il menu artigianale di pulsanti veniva chiuso subito dal compositore su
        // KDE/Wayland).  ⚠️ In GTK3 `set_popup` vuole un `GtkMenu`: passargli un
        // `GtkPopover` fa fallire l'asserzione e il pulsante resta **senza menu**.
        let btn = gtk::MenuButton::new();
        let theme = self.icon_theme();
        if theme.is_some_and(|t| t.has_icon("open-menu-symbolic")) {
            btn.set_image(Some(&gtk::Image::from_icon_name(Some("open-menu-symbolic"), gtk::IconSize::Button)));
        } else {
            btn.set_label("Menu");
        }
        btn.set_tooltip_text(Some("Menu"));
        btn.set_valign(gtk::Align::Center);
        let menu = self.action_menu();
        if std::env::var_os("ZIRKIO_STATS").is_some() {
            btn.connect_toggled(|b| eprintln!("[stats] pulsante ⋮ attivo={}", b.is_active()));
        }
        btn.set_popup(Some(&menu));
        *self.menu_bottone.borrow_mut() = Some(btn.clone());
        self.header.pack_end(&btn);
    }

    fn connect_events(&self) {
        let me = self.me.borrow().clone();
        self.notebook.connect_switch_page(move |_, _, page| {
            if let Some(app) = me.upgrade() {
                app.activate_page(page as usize);
            }
        });
    }

    // ---------------------------------------------------------------- schede

    fn create_view(&self) -> Result<Rc<View>, String> {
        self.create_view_con(None, None)
    }

    /// Crea un pannello; `comando` (programma, argomenti) sostituisce la shell di login,
    /// `titolo` è l'etichetta iniziale della scheda (es. il nome di una sessione SSH).
    fn create_view_con(
        &self,
        comando: Option<(String, Vec<String>)>,
        titolo: Option<String>,
    ) -> Result<Rc<View>, String> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let fonts = self.fonts.borrow().clone();
        let me = self.me.borrow().clone();
        let scale = self.window.scale_factor();
        let cfg = self.cfg.borrow();
        let shortcut_me = me.clone();
        let view = View::new(
            id,
            &cfg,
            fonts,
            me,
            scale,
            Rc::new(move |action| match shortcut_me.upgrade() {
                Some(app) => app.shortcut(action),
                None => false,
            }),
            comando,
            titolo,
        )?;
        self.views.borrow_mut().insert(id, view.clone());
        Ok(view)
    }

    fn new_tab(&self) -> Result<(), String> {
        self.new_tab_con(None, None)
    }

    fn new_tab_con(
        &self,
        comando: Option<(String, Vec<String>)>,
        titolo: Option<String>,
    ) -> Result<(), String> {
        let view = self.create_view_con(comando, titolo.clone())?;
        if let Some(t) = titolo {
            view.label.set_text(&t);
        }
        let label = self.tab_label(&view);
        let page = self.notebook.append_page(&view.area, Some(&label));
        self.notebook.set_tab_reorderable(&view.area, true);
        self.tabs
            .borrow_mut()
            .push(TabInfo { root: view.area.clone().upcast(), views: vec![view.id] });
        self.active.set(view.id);
        // Senza questo la pagina nuova resta invisibile (la finestra ha già fatto show_all).
        view.area.show_all();
        view.label.show();
        view.tab_box.show_all();
        self.notebook.set_current_page(Some(page));
        view.area.grab_focus();
        self.refresh_titles();
        Ok(())
    }

    fn tab_label(&self, view: &Rc<View>) -> gtk::Box {
        let close = gtk::Button::with_label("x");
        close.set_relief(gtk::ReliefStyle::None);
        close.set_focus_on_click(false);
        close.set_tooltip_text(Some("Chiudi"));
        {
            let me = self.me.borrow().clone();
            let id = view.id;
            close.connect_clicked(move |_| {
                if let Some(app) = me.upgrade() {
                    app.close_view(id);
                }
            });
        }
        view.tab_box.pack_end(&close, false, false, 0);
        // Larghezza minima: con più schede la striscia comprime le etichette e una
        // scheda nuova può ridursi a una striscia invisibile.
        view.tab_box.set_size_request(140, -1);
        view.tab_box.set_hexpand(false);
        view.label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        view.tab_box.show_all();
        view.tab_box.clone()
    }

    fn page_index_of(&self, id: u64) -> Option<usize> {
        self.tabs.borrow().iter().position(|t| t.views.contains(&id))
    }

    fn activate_page(&self, page: usize) {
        // Tornando su una scheda: fuoco alla sua area e ridisegno completo
        // (il contesto GL della pagina è stato ricreato al ritorno).
        if let Some(tab) = self.tabs.borrow().get(page) {
            let ids = tab.views.clone();
            if let Some(id) = ids.first().copied() {
                self.active.set(id);
                if let Some(v) = self.views.borrow().get(&id) {
                    v.forced_repaint();
                    // Il fuoco va dato DOPO che la pagina è mappata: chiamato qui dentro
                    // (durante `switch-page`) resta sull'area della scheda nascosta e i
                    // tasti finiscono nella shell invisibile.
                    let v2 = v.clone();
                    glib::idle_add_local_once(move || {
                        v2.area.grab_focus();
                    });
                }
            }
        }
        let view = {
            let tabs = self.tabs.borrow();
            let Some(tab) = tabs.get(page) else { return };
            let Some(id) = tab.views.first().copied() else { return };
            self.views.borrow().get(&id).cloned()
        };
        if let Some(v) = view {
            self.active.set(v.id);
            v.area.grab_focus();
            self.update_subtitle();
        }
    }

    /// Copia della configurazione in uso.
    pub fn config(&self) -> Config {
        self.cfg.borrow().clone()
    }

    /// Righe di diagnostica per la finestra Configurazione.
    pub fn diagnostica(&self) -> String {
        let f = self.fonts.borrow();
        format!(
            "Zirkio Terminal {}\n{}\ncarattere: {}\nfile: {}\ncella: {:.0}x{:.0} px\n\
             schede: {} · pannelli: {}",
            env!("CARGO_PKG_VERSION"),
            if f.has_bold_face() { "neretto: vero" } else { "neretto: finto" },
            f.family,
            f.path,
            f.cell_w,
            f.cell_h,
            self.tabs.borrow().len(),
            self.views.borrow().len()
        )
    }

    /// Applica una configurazione a caldo: carattere, tema, margine, cronologia,
    /// forma del cursore, opacità.  Le modifiche che richiedono una finestra nuova
    /// (dimensioni) valgono dal prossimo avvio.
    pub fn apply_config(&self, nuovo: Config) {
        let mut nuovo = nuovo;
        nuovo.normalizza();
        let vecchio = self.cfg.borrow().clone();
        let font_cambiato = vecchio.font.family != nuovo.font.family
            || (vecchio.font.size - nuovo.font.size).abs() > 0.001
            || (vecchio.font.line_height - nuovo.font.line_height).abs() > 0.001
            || (vecchio.font.bold_strength - nuovo.font.bold_strength).abs() > 0.001;

        if font_cambiato {
            match crate::render::font::FontSet::load(
                nuovo.font.family.as_deref(),
                nuovo.font.size,
                nuovo.font.line_height,
                nuovo.font.bold_strength,
            ) {
                Ok(fs) => {
                    let fs = Rc::new(fs);
                    *self.fonts.borrow_mut() = fs.clone();
                    for v in self.views.borrow().values() {
                        v.set_fonts(fs.clone());
                    }
                }
                Err(e) => eprintln!("[prefs] carattere non caricato: {e}"),
            }
        }
        let tema_cambiato = vecchio.theme.bg != nuovo.theme.bg
            || vecchio.theme.fg != nuovo.theme.fg
            || vecchio.theme.name != nuovo.theme.name;
        if tema_cambiato {
            for v in self.views.borrow().values() {
                v.retint(&vecchio.theme, &nuovo.theme);
            }
        }
        if (vecchio.window.opacity - nuovo.window.opacity).abs() > 0.001 {
            self.window.set_opacity(nuovo.window.opacity as f64);
        }
        let padding_cambiato = vecchio.padding != nuovo.padding;
        let cronologia_cambiata = vecchio.scrollback != nuovo.scrollback;
        for v in self.views.borrow().values() {
            if padding_cambiato {
                v.set_padding(nuovo.padding);
            }
            if cronologia_cambiata {
                v.set_scrollback(nuovo.scrollback);
            }
        }
        *self.cfg.borrow_mut() = nuovo;
    }

    /// Il pannello che ha il fuoco diventa quello su cui agiscono i comandi.
    /// Il processo di un pannello è terminato (`exit`): il pannello si chiude.
    pub fn view_exited(&self, id: u64) {
        if std::env::var_os("ZIRKIO_STATS").is_some() {
            eprintln!("[stats] processo finito nel pannello {id}");
        }
        if self.views.borrow().len() <= 1 {
            self.window.close();
        } else {
            self.close_view(id);
        }
    }

    pub fn set_active(&self, id: u64) {
        if std::env::var_os("ZIRKIO_STATS").is_some() && self.active.get() != id {
            eprintln!("[stats] fuoco sul pannello {id} (era {})", self.active.get());
        }
        if self.active.get() != id && self.views.borrow().contains_key(&id) {
            self.active.set(id);
            self.update_subtitle();
        }
    }

    pub fn active_view(&self) -> Option<Rc<View>> {
        self.views.borrow().get(&self.active.get()).cloned()
    }

    fn activate(&self, id: u64) {
        if let Some(v) = self.views.borrow().get(&id).cloned() {
            self.active.set(id);
            v.area.grab_focus();
            self.update_subtitle();
        }
    }

    // ------------------------------------------------------------- pannelli

    fn split(&self, orient: gtk::Orientation) {
        let Some(view) = self.active_view() else { return };
        let new_view = match self.create_view() {
            Ok(v) => v,
            Err(e) => {
                self.error(&e);
                return;
            }
        };
        let paned = gtk::Paned::new(orient);
        paned.set_wide_handle(true);
        // NIENTE set_position: se il pannello non è ancora allocato vale 0 e la
        // divisione nasce con la barra a zero (sembrava che "non lo facesse").
        paned.set_resize_mode(gtk::ResizeMode::Queue);
        if !self.substitute(&view.area.clone().upcast::<gtk::Widget>(), &paned.clone().upcast()) {
            eprintln!(
                "zirkio-term: divisione impossibile (contenitore: {:?})",
                view.area.parent().map(|p| p.type_().name().to_string())
            );
            return;
        }
        paned.pack1(&view.area, true, true);
        paned.pack2(&new_view.area, true, true);
        paned.show_all();

        if let Some(idx) = self.page_index_of(view.id) {
            let mut tabs = self.tabs.borrow_mut();
            if let Some(tab) = tabs.get_mut(idx) {
                tab.root = paned.clone().upcast();
                tab.views.push(new_view.id);
            }
        }
        self.active.set(new_view.id);
        new_view.area.grab_focus();
    }

    /// Mette `new` al posto di `old` nel contenitore che lo ospita.
    fn substitute(&self, old: &gtk::Widget, new: &gtk::Widget) -> bool {
        let Some(parent) = old.parent() else { return false };
        if let Ok(paned) = parent.clone().downcast::<gtk::Paned>() {
            let first = paned.child1().map(|c| c == *old).unwrap_or(false);
            paned.remove(old);
            if first {
                paned.pack1(new, true, true);
            } else {
                paned.pack2(new, true, true);
            }
            new.show_all();
            true
        } else if let Ok(nb) = parent.clone().downcast::<gtk::Notebook>() {
            let page = nb.page_num(old).unwrap_or(0);
            // ⚠️ `remove` della pagina **corrente** fa scegliere a GTK un'altra scheda, e
            // la successiva `insert_page` non la riporta al suo posto: era il difetto per
            // cui, dividendo la scheda nuova, l'app **saltava alla scheda precedente**.
            let era_attiva = nb.current_page() == Some(page);
            let label = nb.tab_label(old);
            nb.remove(old);
            nb.insert_page(new, label.as_ref(), Some(page));
            nb.set_tab_reorderable(new, true);
            new.show_all();
            if era_attiva {
                nb.set_current_page(Some(page));
            }
            true
        } else {
            false
        }
    }

    fn close_view(&self, id: u64) {
        if std::env::var_os("ZIRKIO_STATS").is_some() {
            eprintln!("[stats] chiudo il pannello {id}");
        }
        let Some(view) = self.views.borrow().get(&id).cloned() else { return };
        let Some(idx) = self.page_index_of(id) else { return };
        // Ferma il ciclo del pannello: senza questo la shell resta viva e non salva la
        // cronologia (vedi `View::chiudi`).
        view.chiudi();
        let only = self.tabs.borrow()[idx].views.len() <= 1;
        if only {
            self.close_tab_index(idx);
            return;
        }
        // Il fratello prende il posto della divisione.
        let parent = view.area.parent();
        if let Some(paned) = parent.and_downcast::<gtk::Paned>() {
            let area_widget = view.area.clone().upcast::<gtk::Widget>();
            let first = paned.child1().map(|c| c == area_widget).unwrap_or(false);
            let sibling = if first { paned.child2() } else { paned.child1() };
            paned.remove(&view.area);
            if let Some(sib) = sibling {
                paned.remove(&sib);
                let paned_w: gtk::Widget = paned.clone().upcast();
                self.substitute(&paned_w, &sib);
            }
            self.views.borrow_mut().remove(&id);
            {
                let mut tabs = self.tabs.borrow_mut();
                if let Some(tab) = tabs.get_mut(idx) {
                    tab.views.retain(|v| *v != id);
                }
            }
            if let Some(next) = self.find_view_in_page(idx) {
                self.activate(next);
            }
        }
    }

    fn find_view_in_page(&self, idx: usize) -> Option<u64> {
        self.tabs.borrow().get(idx).and_then(|t| t.views.first().copied())
    }

    fn close_tab_index(&self, idx: usize) {
        let views: Vec<u64> = self.tabs.borrow().get(idx).map(|t| t.views.clone()).unwrap_or_default();
        // Prima si fermano **tutti** i pannelli della scheda (chiudendo la scheda intera
        // altrimenti le loro shell restavano vive e non salvavano la cronologia).
        for id in &views {
            if let Some(v) = self.views.borrow().get(id) {
                v.chiudi();
            }
        }
        self.notebook.remove_page(Some(idx as u32));
        self.tabs.borrow_mut().remove(idx);
        for id in views {
            self.views.borrow_mut().remove(&id);
        }
        if self.tabs.borrow().is_empty() {
            self.window.close();
        }
    }

    fn close_pane(&self) {
        let id = self.active.get();
        self.close_view(id);
    }

    fn focus_step(&self, forward: bool) {
        let Some(idx) = self.page_index_of(self.active.get()) else { return };
        let ids = self.tabs.borrow()[idx].views.clone();
        if ids.len() < 2 {
            return;
        }
        if let Some(pos) = ids.iter().position(|v| *v == self.active.get()) {
            let n = ids.len();
            let next = if forward { ids[(pos + 1) % n] } else { ids[(pos + n - 1) % n] };
            self.activate(next);
        }
    }

    fn cycle_tab(&self, forward: bool) {
        let n = self.notebook.n_pages();
        if n < 2 {
            return;
        }
        let cur = self.notebook.current_page().unwrap_or(0);
        let next = if forward { (cur + 1) % n } else { (cur + n - 1) % n };
        self.notebook.set_current_page(Some(next));
    }

    /// Misure delle schede e dei pannelli (per capire le larghezze a zero).
    pub fn misura_tutto(&self) {
        for v in self.views.borrow().values() {
            eprintln!(
                "[stats] misura vista {}: area {}x{} scheda {}x{} visibile={} mappata={}",
                v.id,
                v.area.allocated_width(),
                v.area.allocated_height(),
                v.tab_box.allocated_width(),
                v.tab_box.allocated_height(),
                v.area.is_visible(),
                v.area.is_mapped()
            );
        }
        eprintln!(
            "[stats] misura notebook: pagine={} cella={}x{}",
            self.notebook.n_pages(),
            self.fonts.borrow().cell_w,
            self.fonts.borrow().cell_h
        );
    }

    /// Esegue passi di prova senza tastiera (vedi `ZIRKIO_SELFTEST`).
    fn selftest(&self, steps: &str) {
        for step in steps.split(',').map(|s| s.trim()) {
            if std::env::var_os("ZIRKIO_STATS").is_some() {
                eprintln!("[stats] selftest: {step}");
            }
            let ok = match step {
                "tab" => self.new_tab().is_ok(),
                "split-right" => {
                    self.split(gtk::Orientation::Horizontal);
                    true
                }
                "split-down" => {
                    self.split(gtk::Orientation::Vertical);
                    true
                }
                "theme" => {
                    self.toggle_theme();
                    true
                }
                s if s.starts_with("sd:") => {
                    // `sd:<nome>`: la conferma di spegnimento (si clicca «Spegni»)
                    self.spegni_sessione(s.trim_start_matches("sd:")).is_ok()
                }
                "prefs" => {
                    if let Some(rc) = self.me.borrow().upgrade() {
                        crate::prefs::apri(&rc);
                    }
                    true
                }
                "config-grande" => {
                    let mut c = self.cfg.borrow().clone();
                    c.font.size = 18.0;
                    self.apply_config(c);
                    true
                }
                "config-tema" => {
                    let mut c = self.cfg.borrow().clone();
                    c.theme = crate::config::Theme::light();
                    self.apply_config(c);
                    true
                }
                "close-pane" => {
                    self.close_pane();
                    true
                }
                "trasferimenti" => {
                    if let Some(rc) = self.me.borrow().upgrade() {
                        crate::scpwin::apri(&rc);
                    }
                    true
                }
                // `scp:<sorgente>|<destinazione>`: mette in coda un trasferimento vero
                // (serve ai collaudi: poi si controlla che il file sia arrivato).
                passo if passo.starts_with("scp:") => {
                    match passo[4..].split_once('|') {
                        Some((a, b)) => match self.me.borrow().upgrade() {
                            Some(rc) => match crate::scpwin::accoda(&rc, a, b, None) {
                                Ok(id) => {
                                    eprintln!("[stats] selftest scp: in coda #{id} {a} -> {b}");
                                    true
                                }
                                Err(e) => {
                                    eprintln!("[stats] selftest scp: {e}");
                                    false
                                }
                            },
                            None => false,
                        },
                        None => {
                            eprintln!("[stats] selftest scp: manca «|» fra sorgente e destinazione");
                            false
                        }
                    }
                }
                other => {
                    eprintln!("[stats] selftest: passo sconosciuto {other}");
                    false
                }
            };
            if std::env::var_os("ZIRKIO_STATS").is_some() {
                let (n_tab, n_view) = (self.tabs.borrow().len(), self.views.borrow().len());
                eprintln!("[stats] selftest: {step} -> {ok} (schede {n_tab}, pannelli {n_view})");
            }
            // misure DOPO che GTK ha impaginato (un ciclo dopo)
            glib::timeout_add_local_once(std::time::Duration::from_millis(120), {
                let me = self.me.borrow().clone();
                move || {
                    if let Some(a) = me.upgrade() {
                        a.misura_tutto();
                    }
                }
            });
        }
    }

    // ---------------------------------------------------------------- azioni

    pub fn shortcut(&self, action: Action) -> bool {
        // Le azioni che creano o distruggono widget NON si eseguono dentro il segnale
        // (menù aperto, tasto premuto): si rimandano al ciclo principale. Dentro un
        // `GtkMenu` in chiusura GTK non gradisce widget nuovi e non succede niente.
        let deferred = matches!(
            action,
            Action::NewTab | Action::SplitRight | Action::SplitDown | Action::ClosePane | Action::Quit
        );
        if deferred {
            let me = self.me.borrow().clone();
            glib::idle_add_local_once(move || {
                if let Some(app) = me.upgrade() {
                    app.run_action_now(action);
                }
            });
            return true;
        }
        self.run_action_now(action);
        true
    }

    /// Esegue subito l'azione (senza rimandarla al ciclo principale).
    fn run_action_now(&self, action: Action) {
        match action {
            Action::NewTab => {
                if let Err(e) = self.new_tab() {
                    self.error(&e);
                }
            }
            Action::ClosePane => self.close_pane(),
            Action::SplitRight => self.split(gtk::Orientation::Horizontal),
            Action::SplitDown => self.split(gtk::Orientation::Vertical),
            Action::Copy => {
                if let Some(v) = self.active_view() {
                    v.copy();
                }
            }
            Action::Paste => {
                if let Some(v) = self.active_view() {
                    v.paste();
                }
            }
            Action::SelectAll => {
                if let Some(v) = self.active_view() {
                    v.select_all();
                }
            }
            Action::FontBigger => self.change_font(1.0),
            Action::FontSmaller => self.change_font(-1.0),
            Action::FontReset => {
                self.cfg.borrow_mut().font.size = 13.0;
                self.reload_fonts();
            }
            Action::ThemeNext => self.toggle_theme(),
            Action::NextTab => self.cycle_tab(true),
            Action::PrevTab => self.cycle_tab(false),
            Action::FocusNext => self.focus_step(true),
            Action::FocusPrev => self.focus_step(false),
            Action::ScrollPageUp => {
                if let Some(v) = self.active_view() {
                    v.scroll_page(true);
                }
            }
            Action::ScrollPageDown => {
                if let Some(v) = self.active_view() {
                    v.scroll_page(false);
                }
            }
            Action::ScrollTop => {
                if let Some(v) = self.active_view() {
                    v.scroll_end(false);
                }
            }
            Action::ScrollBottom => {
                if let Some(v) = self.active_view() {
                    v.scroll_end(true);
                }
            }
            Action::Help => self.help_dialog(),
            Action::Prefs => {
                // Creare una finestra dentro il gestore del menu non piace a GTK:
                // la si apre al giro dopo.
                if let Some(rc) = self.me.borrow().upgrade() {
                    glib::idle_add_local_once(move || crate::prefs::apri(&rc));
                }
            }
            Action::SessioniSsh => {
                // Come per la configurazione: finestra nuova al giro dopo.
                if let Some(rc) = self.me.borrow().upgrade() {
                    glib::idle_add_local_once(move || crate::sshwin::apri(&rc));
                }
            }
            Action::Trasferimenti => {
                if let Some(rc) = self.me.borrow().upgrade() {
                    glib::idle_add_local_once(move || {
                        crate::scpwin::apri(&rc);
                    });
                }
            }
            Action::Quit => self.window.close(),
        }
        if std::env::var_os("ZIRKIO_STATS").is_some() {
            eprintln!(
                "[stats] azione {:?}: schede={} pannelli={} attivo={} pagine={} corrente={:?}",
                action,
                self.tabs.borrow().len(),
                self.views.borrow().len(),
                self.active.get(),
                self.notebook.n_pages(),
                self.notebook.current_page()
            );
            if let Some(v) = self.active_view() {
                eprintln!(
                    "[stats]   vista attiva {}: visibile={} mappata={} larghezza={}",
                    v.id,
                    v.area.is_visible(),
                    v.area.is_mapped(),
                    v.area.allocated_width()
                );
            }
        }
    }

    fn change_font(&self, delta: f32) {
        let size = (self.cfg.borrow().font.size + delta).clamp(6.0, 48.0);
        self.cfg.borrow_mut().font.size = size;
        self.reload_fonts();
    }

    fn reload_fonts(&self) {
        let scale = self.window.scale_factor() as f32;
        let loaded = {
            let cfg = self.cfg.borrow();
            FontSet::load(
                cfg.font.family.as_deref(),
                cfg.font.size * scale,
                cfg.font.line_height,
                cfg.font.bold_strength,
            )
        };
        match loaded {
            Ok(f) => {
                let fonts = Rc::new(f);
                *self.fonts.borrow_mut() = fonts.clone();
                for v in self.views.borrow().values() {
                    v.set_fonts(fonts.clone());
                }
            }
            Err(e) => self.error(&e),
        }
    }

    fn toggle_theme(&self) {
        let old = self.cfg.borrow().theme.clone();
        let name = old.name.to_ascii_lowercase();
        let new = if name.contains("dark") { Theme::light() } else { Theme::dark() };
        self.cfg.borrow_mut().theme = new.clone();
        for v in self.views.borrow().values() {
            v.retint(&old, &new);
        }
    }

    // ---------------------------------------------------------------- sessioni SSH

    /// Le sessioni SSH salvate (copia, per la finestra di gestione).
    pub fn sessioni(&self) -> Vec<crate::ssh::Sessione> {
        self.sessioni.borrow().clone()
    }

    /// Sostituisce l'elenco: memoria, file su disco e voce di menu.
    pub fn salva_sessioni(&self, nuove: &[crate::ssh::Sessione]) -> Result<(), String> {
        crate::ssh::salva(nuove)?;
        *self.sessioni.borrow_mut() = nuove.to_vec();
        self.ricostruisci_menu();
        Ok(())
    }

    /// Rifà il menu del ⋮: le sessioni compaiono come voci del sottomenu «SSH».
    pub fn ricostruisci_menu(&self) {
        if let Some(btn) = self.menu_bottone.borrow().clone() {
            btn.set_popup(Some(&self.action_menu()));
        }
        if let Some(btn) = self.menu_ssh_bottone.borrow().clone() {
            btn.set_popup(Some(&self.menu_ssh()));
        }
    }

    /// Vero se esiste una sessione con questo nome (per `@sh <nome>`).
    pub fn ha_sessione(&self, nome: &str) -> bool {
        self.sessioni.borrow().iter().any(|s| s.nome == nome)
    }

    /// Mette in coda un trasferimento (`@scp`): `<sorgente> <destinazione>`.
    ///
    /// Accetta le due forme di sempre: percorso locale, oppure `sessione:percorso`.
    /// Il tipo (file o cartella) lo scopre `scp::rileva` in un thread.
    pub fn trasferisci(
        &self,
        sorgente: &str,
        destinazione: &str,
        cwd: Option<&std::path::Path>,
    ) -> Result<(), String> {
        let rc = self
            .me
            .borrow()
            .upgrade()
            .ok_or("l'applicazione non è disponibile")?;
        crate::scpwin::accoda(&rc, sorgente, destinazione, cwd).map(|_| ())
    }

    /// Mostra un errore in una finestra (per chi chiama da fuori del modulo).
    pub fn errore_pubblico(&self, msg: &str) {
        self.error(msg);
    }

    /// Apre una sessione salvata in una scheda nuova.
    ///
    /// Se la sessione ha il segno «invia la chiave», il segno si **toglie** dopo l'uso:
    /// la chiave si installa una volta sola, poi si entra senza password (se qualcosa va
    /// storto si riaccende il toggle nella finestra delle sessioni).
    /// Spegne una macchina salvata (`@sd <nome>`).
    ///
    /// Chiede conferma e poi apre una scheda con `ssh -t … sudo shutdown -h now`: la
    /// scheda serve perché `sudo` possa chiedere la password **lì**, dove la si vede —
    /// e dove l'inserimento automatico la dà da solo, se per quella macchina la
    /// conosciamo.  Quando la macchina si spegne il collegamento cade e la scheda
    /// si chiude da sola.
    pub fn spegni_sessione(&self, nome: &str) -> Result<(), String> {
        let nome = nome.trim();
        let sessione = self
            .sessioni
            .borrow()
            .iter()
            .find(|s| s.nome.eq_ignore_ascii_case(nome))
            .cloned()
            .ok_or_else(|| {
                let nomi: Vec<String> = self
                    .sessioni
                    .borrow()
                    .iter()
                    .map(|s| s.nome.clone())
                    .collect();
                if nomi.is_empty() {
                    format!("nessuna macchina salvata: «{nome}» non esiste")
                } else {
                    format!("nessuna macchina «{nome}»: ci sono {}", nomi.join(", "))
                }
            })?;
        let rc = self
            .me
            .borrow()
            .upgrade()
            .ok_or("l'applicazione non è disponibile")?;
        let d = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Question,
            gtk::ButtonsType::None,
            &format!(
                "Spegnere «{}»?\n\n{}@{} · porta {}",
                sessione.nome, sessione.utente, sessione.host, sessione.porta
            ),
        );
        d.set_title("Spegni la macchina — Zirkio Terminal");
        d.add_button("Annulla", gtk::ResponseType::Cancel);
        d.add_button("Spegni", gtk::ResponseType::Accept);
        // Il tasto per difetto è «Annulla»: un Invio di troppo non spegne niente.
        d.set_default_response(gtk::ResponseType::Cancel);
        let s = sessione.clone();
        d.connect_response(move |d, risposta| {
            d.close();
            if risposta != gtk::ResponseType::Accept {
                return;
            }
            let (prog, args) = s.spegni();
            if std::env::var_os("ZIRKIO_STATS").is_some() {
                eprintln!("[stats] @sd: {prog} {}", args.join(" "));
            }
            if let Err(e) = rc.new_tab_con(Some((prog, args)), Some(format!("spegni {}", s.nome))) {
                rc.errore_pubblico(&e);
            }
        });
        d.show_all();
        Ok(())
    }

    pub fn apri_sessione(&self, nome: &str) -> Result<(), String> {
        let (sessione, primo_invio) = {
            let v = self.sessioni.borrow();
            let s = v
                .iter()
                .find(|s| s.nome == nome)
                .cloned()
                .ok_or_else(|| format!("nessuna sessione SSH chiamata «{nome}»"))?;
            (s.clone(), s.invia_chiave)
        };
        let (prog, args) = sessione.comando();
        if std::env::var_os("ZIRKIO_STATS").is_some() {
            eprintln!("[stats] ssh: {prog} {}", args.join(" "));
        }
        self.new_tab_con(Some((prog, args)), Some(sessione.nome.clone()))?;
        // Il segno «invia la chiave» **non** si toglie dopo l'uso: il comando comincia
        // con un tentativo `ssh -o BatchMode=yes`, quindi quando la chiave è installata
        // non costa niente (nessuna password, nessuna copia).  Toglierlo subito era il
        // difetto: se la copia falliva, il toggle restava spento e la password tornava.
        let _ = primo_invio;
        Ok(())
    }

    fn update_subtitle(&self) {
        let title = self.active_view().map(|v| v.title()).unwrap_or_default();
        let sub = if title.is_empty() { "terminale".to_string() } else { title };
        self.header.set_subtitle(Some(&sub));
    }

    /// Aggiorna le etichette delle schede quando il processo cambia titolo (OSC 0/2).
    pub fn refresh_titles(&self) {
        let views: Vec<(u64, Rc<View>)> = self.views.borrow().iter().map(|(k, v)| (*k, v.clone())).collect();
        for (id, v) in views {
            let title = v.title();
            let changed = {
                let mut shown = self.title_shown.borrow_mut();
                match shown.get(&id) {
                    Some(t) if *t == title => false,
                    _ => {
                        shown.insert(id, title.clone());
                        true
                    }
                }
            };
            if changed {
                // Le schede nate da una sessione salvata tengono il **nome della
                // connessione**: il titolo mandato dal programma remoto (OSC 0/2) va
                // solo nel sottotitolo della finestra.
                if !v.titolo_fisso {
                    v.label.set_text(&title);
                }
            }
        }
        self.update_subtitle();
    }

    fn error(&self, msg: &str) {
        let d = gtk::MessageDialog::new(
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Error,
            gtk::ButtonsType::Close,
            msg,
        );
        d.connect_response(|d, _| d.close());
        d.show_all();
    }

    fn help_dialog(&self) {
        let d = gtk::Dialog::with_buttons(
            Some("Scorciatoie"),
            Some(&self.window),
            gtk::DialogFlags::MODAL,
            &[("Chiudi", gtk::ResponseType::Close)],
        );
        d.set_default_size(460, 420);
        let grid = gtk::Grid::new();
        grid.set_column_spacing(24);
        grid.set_row_spacing(6);
        grid.set_margin_top(16);
        grid.set_margin_bottom(16);
        grid.set_margin_start(18);
        grid.set_margin_end(18);
        for (row, (keys, what)) in crate::SHORTCUTS.iter().enumerate() {
            let k = gtk::Label::new(None);
            k.set_markup(&format!("<tt>{}</tt>", glib::markup_escape_text(keys)));
            k.set_xalign(0.0);
            let w = gtk::Label::new(Some(what));
            w.set_xalign(0.0);
            grid.attach(&k, 0, row as i32, 1, 1);
            grid.attach(&w, 1, row as i32, 1, 1);
        }
        grid.show_all();
        d.content_area().add(&grid);
        d.connect_response(|d, _| d.close());
        d.show_all();
    }
}

/// Riga di una voce di menu: etichetta a sinistra (larghezza fissa, così tutte le
/// scorciatoie cadono nella stessa colonna) e scorciatoia a destra in grigio.
/// Larghezza massima (in caratteri) della colonna delle etichette in un menu.
///
/// Oltre questa misura la voce si **tronca con «…»**: una descrizione lunga non deve
/// allargare il menu a tutta la finestra (il testo intero resta nel suggerimento).
const MAX_CARATTERI_VOCE: i32 = 42;

/// Caratteri di descrizione mostrati nel menu (oltre si tronca con «…»).
const MAX_CARATTERI_DESCRIZIONE: i32 = 26;

/// Riga di una sessione SSH nel menu: **nome** e, accanto, la descrizione in grigio
/// piccolo, troncata se lunga (il testo intero resta nel suggerimento).
fn riga_sessione(nome: &str, descrizione: &str) -> gtk::Box {
    let riga = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    let n = gtk::Label::new(Some(nome));
    n.set_xalign(0.0);
    riga.pack_start(&n, false, false, 0);
    let d_testo = descrizione.trim();
    if !d_testo.is_empty() {
        let d = gtk::Label::new(None);
        d.set_markup(&format!(
            "<span size=\"small\" alpha=\"60%\">{}</span>",
            gtk::glib::markup_escape_text(d_testo)
        ));
        d.set_xalign(0.0);
        d.set_ellipsize(gtk::pango::EllipsizeMode::End);
        d.set_max_width_chars(MAX_CARATTERI_DESCRIZIONE);
        d.set_single_line_mode(true);
        if d_testo.chars().count() as i32 > MAX_CARATTERI_DESCRIZIONE {
            d.set_tooltip_text(Some(d_testo));
        }
        riga.pack_start(&d, false, false, 0);
    }
    riga.show_all();
    riga
}

fn riga_voce(testo: &str, scorciatoia: &str) -> gtk::Box {
    let riga = gtk::Box::new(gtk::Orientation::Horizontal, 28);
    let l = gtk::Label::new(Some(testo));
    l.set_xalign(0.0);
    // Ogni etichetta prende la larghezza **del suo testo** (il menu si adatta al
    // contenuto), con un tetto: oltre i `MAX_CARATTERI_VOCE` caratteri si tronca con
    // «…» e il testo intero resta nel suggerimento.
    l.set_max_width_chars(MAX_CARATTERI_VOCE);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l.set_single_line_mode(true);
    if testo.chars().count() as i32 > MAX_CARATTERI_VOCE {
        l.set_tooltip_text(Some(testo));
    }
    riga.pack_start(&l, true, true, 0);
    if !scorciatoia.is_empty() {
        let a = gtk::Label::new(None);
        a.set_markup(&format!(
            "<span size=\"small\" alpha=\"55%\">{}</span>",
            gtk::glib::markup_escape_text(scorciatoia)
        ));
        a.set_xalign(1.0);
        riga.pack_end(&a, false, false, 0);
    }
    riga.show_all();
    riga
}


/// Separatore **visibile** fra due gruppi di voci: una riga disegnata da noi.
///
/// Il `GtkSeparatorMenuItem` del tema è una linea di 1 px quasi invisibile (segnalato
/// dall'utente: «non si vedono le righe di separazione»), quindi si mette un
/// `GtkSeparator` con margini dentro una voce non sensibile e uno stile dedicato.
fn separatore_menu() -> gtk::MenuItem {
    let item = gtk::MenuItem::new();
    let s = gtk::Separator::new(gtk::Orientation::Horizontal);
    s.set_margin_top(6);
    s.set_margin_bottom(6);
    s.set_margin_start(2);
    s.set_margin_end(2);
    s.style_context().add_class("sep-menu");
    item.add(&s);
    item.set_sensitive(false);
    item.set_can_focus(false);
    item
}

/// Registra (una volta sola) lo stile dei separatori di menu.
pub fn installa_stile_menu() {
    use std::sync::Once;
    static UNA: Once = Once::new();
    UNA.call_once(|| {
        let prov = gtk::CssProvider::new();
        prov.load_from_data(
            b"separator.sep-menu { background-color: alpha(currentColor, 0.45); min-height: 1px; }\
              menuitem { padding-top: 3px; padding-bottom: 3px; }",
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

