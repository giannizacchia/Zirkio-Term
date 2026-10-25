//! Un pannello di terminale: `GtkGLArea` + motore del terminale + pty.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use gtk::glib;
use gtk::prelude::*;

use crate::app::Action;
use crate::config::Config;
use crate::pty::{Pty, Sink};
use crate::render::font::FontSet;
use crate::render::{GlLoaderOwned, Renderer};
use crate::term::input::{self, Mods, MouseKind};
use crate::term::{Point, Terminal};

/// Messaggi dal thread del pty verso il ciclo grafico.
pub enum Msg {
    Data(Vec<u8>),
    Exit,
    /// `sudo` chiede la password e non ce n'è nessuna per quella macchina: la
    /// finestra si apre nel ciclo grafico (un thread non può toccare i widget).
    SudoSenzaPassword(crate::pty::Maniglia, crate::sudo::Bersaglio),
}

pub struct Sel {
    pub anchor: Point,
    pub head: Point,
}

pub struct ViewState {
    pub term: Terminal,
    parser: vte::Parser,
    pub pty: Pty,
    pub renderer: Option<Renderer>,
    /// Tenuto vivo perché il contesto GL usa i suoi puntatori.
    _loader: Option<GlLoaderOwned>,
    pub sel: Option<Sel>,
    dragging: bool,
    last_cursor: (usize, usize),
    pub full_dirty: bool,
    bench_done: bool,
    /// Istante di creazione: serve a `ZIRKIO_PPM_DOPO` (fotogramma dopo N ms).
    started: std::time::Instant,
    /// Buffer riusati fra un fotogramma e l'altro (evita una `Vec` nuova ogni volta).
    scratch: Vec<crate::render::Instance>,
    overlay: Vec<crate::render::Instance>,
    riga_vuota: Vec<crate::term::Cell>,
    pub cols: usize,
    pub rows: usize,
    pub padding: f32,
    pub alive: bool,
    pub exited: bool,
    ppm_done: bool,
    /// Carattere nuovo da applicare **al prossimo fotogramma**: l'atlante dei glifi si
    /// tocca solo con il contesto OpenGL corrente (dentro `connect_render`), altrimenti
    /// le `tex_sub_image` sono no-op e le celle finiscono per mostrare glifi sbagliati.
    pending_fonts: Option<Rc<FontSet>>,
    /// Il carattere **in uso** (aggiornato anche a caldo), per rifare il renderer.
    fonts: Rc<FontSet>,
    /// I caratteri digitati nella riga corrente all'ultimo prompt: servono a riconoscere
    /// `@sh <nome>` **prima** di mandare l'Invio alla shell.
    riga_digitata: String,
    /// Riconosce il prompt di `sudo` nel flusso del programma.
    rilevatore: crate::sudo::Rilevatore,
    /// Un tasto premuto dall'utente annulla l'invio automatico della password
    /// (il thread che aspetta l'eco spento guarda questo, non il ciclo grafico).
    tasto: Arc<AtomicBool>,
}

pub struct View {
    pub id: u64,
    pub area: gtk::GLArea,
    pub state: Rc<RefCell<ViewState>>,
    /// Etichetta della scheda (aggiornata dal titolo OSC, salvo `titolo_fisso`).
    pub label: gtk::Label,
    /// La scheda è nata da una sessione salvata (SSH): l'etichetta **resta** il nome
    /// della connessione anche se il programma remoto manda un titolo suo (OSC 0/2).
    pub titolo_fisso: bool,
    pub tab_box: gtk::Box,
}

impl View {
    /// Ridisegno completo (dopo che il contesto GL è stato ricreato, o tornando su una scheda).
    pub fn forced_repaint(&self) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            st.full_dirty = true;
        }
        self.area.queue_render();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: u64,
        cfg: &Config,
        fonts: Rc<FontSet>,
        app: std::rc::Weak<crate::app::App>,
        scale: i32,
        on_shortcut: Rc<dyn Fn(Action) -> bool>,
        // Programma da lanciare al posto della shell di login (es. `ssh`).
        comando: Option<(String, Vec<String>)>,
        // Titolo iniziale della scheda (es. il nome della sessione SSH).
        titolo: Option<String>,
    ) -> Result<Rc<View>, String> {
        let area = gtk::GLArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_can_focus(true);
        area.set_has_depth_buffer(false);
        area.set_has_stencil_buffer(false);
        area.set_auto_render(true);
        area.add_events(
            gdk::EventMask::KEY_PRESS_MASK
                | gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::BUTTON_RELEASE_MASK
                | gdk::EventMask::POINTER_MOTION_MASK
                | gdk::EventMask::SCROLL_MASK
                | gdk::EventMask::SMOOTH_SCROLL_MASK,
        );

        let padding = cfg.padding as f32 * scale as f32;
        let mut term = Terminal::new(
            80,
            24,
            cfg.scrollback,
            cfg.theme.palette256(),
            cfg.theme.fg,
            cfg.theme.bg,
        );
        // Il titolo iniziale (nome della sessione SSH) resta finché il programma non
        // manda un titolo suo (OSC 0/2).
        if let Some(t) = &titolo {
            term.title = t.clone();
        }
        // La scheda di una sessione salvata tiene il nome della connessione: il titolo
        // che il programma remoto manda (OSC 0/2) non la rinomina.
        let titolo_fisso = titolo.is_some();

        // Alzato da qualunque tasto: il thread di sudo annulla l'invio automatico.
        let tasto = Arc::new(AtomicBool::new(false));

        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = mpsc::channel();
        // Una copia per il ciclo grafico: serve a rimandare indietro la richiesta di
        // password quando non ce n'è nessuna da usare (il `tx` principale va al pty).
        let tx_sudo = tx.clone();
        let (shell, args) = match comando.clone() {
            Some((prog, a)) => (prog, a),
            None => cfg.shell_command(),
        };
        let tx_exit = tx.clone();
        let sink = Sink {
            on_data: Box::new(move |data| {
                let _ = tx.send(Msg::Data(data));
            }),
            on_exit: Box::new(move || {
                let _ = tx_exit.send(Msg::Exit);
            }),
        };
        let pty = Pty::spawn(&shell, &args, 80, 24, None, sink)
            .map_err(|e| format!("impossibile avviare {shell}: {e}"))?;

        let state = Rc::new(RefCell::new(ViewState {
            term,
            parser: vte::Parser::new(),
            pty,
            renderer: None,
            _loader: None,
            sel: None,
            dragging: false,
            last_cursor: (usize::MAX, usize::MAX),
            full_dirty: true,
            bench_done: false,
            started: std::time::Instant::now(),
            scratch: Vec::new(),
            overlay: Vec::new(),
            riga_vuota: Vec::new(),
            cols: 80,
            rows: 24,
            padding,
            alive: true,
            exited: false,
            ppm_done: false,
            pending_fonts: None,
            fonts: fonts.clone(),
            riga_digitata: String::new(),
            rilevatore: crate::sudo::Rilevatore::new(),
            tasto: tasto.clone(),
        }));

        // --- contesto OpenGL creato da GTK
        {
            let st = state.clone();
            area.connect_realize(move |area| {
                area.make_current();
                let mut st = st.borrow_mut();
                // Il carattere **in uso** (non quello dell'avvio): cambiando carattere e
                // poi cambiando scheda il renderer si rifà, e deve ripartire da quello nuovo.
                let fonts = st.fonts.clone();
                match GlLoaderOwned::new() {
                    Ok(loader) => {
                        let gl = loader.loader.context();
                        match Renderer::new(gl, fonts.clone()) {
                            Ok(mut r) => {
                                r.atlas.prewarm(&r.gl, &r.fonts);
                                st.renderer = Some(r);
                                st._loader = Some(loader);
                                st.full_dirty = true;
                            }
                            Err(e) => eprintln!("zirkio-term: renderer: {e}"),
                        }
                    }
                    Err(e) => eprintln!("zirkio-term: OpenGL: {e}"),
                }
            });
        }
        {
            let st = state.clone();
            area.connect_unrealize(move |_| {
                // La pagina è solo nascosta (cambio scheda): il contesto OpenGL però
                // non vale più, quindi si butta il renderer.  `alive` NON va toccato:
                // prima metteva `alive = false` e il terminale di quella scheda
                // restava muto per sempre (nessun prompt al ritorno).
                let mut st = st.borrow_mut();
                st.renderer = None;
                st._loader = None;
                st.ppm_done = false;
            });
        }

        // --- disegno
        {
            let st = state.clone();
            area.connect_render(move |area, _ctx| {
                let mut st = st.borrow_mut();
                if st.renderer.is_none() {
                    return glib::Propagation::Proceed;
                }
                area.attach_buffers();
                let scale = area.scale_factor() as f32;
                let w = (area.allocated_width() as f32 * scale).round() as i32;
                let h = (area.allocated_height() as f32 * scale).round() as i32;
                // Banco di prova: `ZIRKIO_BENCH=200` misura il costo CPU di un
                // fotogramma completo (costruzione istanze + carico + disegno).
                if !st.bench_done {
                    if let Ok(v) = std::env::var("ZIRKIO_BENCH") {
                        if let Ok(n) = v.parse::<u32>() {
                            if n > 0 {
                                let t0 = std::time::Instant::now();
                                for _ in 0..n {
                                    st.full_dirty = true;
                                    st.sync_grid(w, h);
                                    st.paint(w, h);
                                }
                                let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
                                eprintln!(
                                    "[bench] {n} fotogrammi completi: {ms:.3} ms a fotogramma ({w}x{h} px, {}x{} celle)",
                                    st.cols, st.rows
                                );
                                st.bench_done = true;
                            }
                        }
                    }
                }
                st.sync_grid(w, h);
                st.paint(w, h);
                // Fotogramma su file per i collaudi.  `ZIRKIO_PPM_DOPO=<ms>` fa
                // aspettare: al primo fotogramma lo schermo è ancora vuoto.
                if !st.ppm_done {
                    if let Some(path) = std::env::var_os("ZIRKIO_PPM") {
                        let dopo = std::env::var("ZIRKIO_PPM_DOPO")
                            .ok()
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(0);
                        if st.started.elapsed().as_millis() as u64 >= dopo {
                            if let Some(r) = st.renderer.as_ref() {
                                if let Err(e) = r.write_ppm(std::path::Path::new(&path)) {
                                    eprintln!("zirkio-term: PPM: {e}");
                                }
                            }
                            st.ppm_done = true;
                        }
                    }
                }
                glib::Propagation::Stop
            });
        }
        {
            let st = state.clone();
            area.connect_resize(move |area, _w, _h| {
                st.borrow_mut().full_dirty = true;
                area.queue_render();
            });
        }

        // Fotogramma su file: se c'è un ritardo richiesto, si forza un disegno
        // all'ora giusta (l'app disegna solo quando qualcosa cambia).
        if std::env::var_os("ZIRKIO_PPM").is_some() {
            let dopo = std::env::var("ZIRKIO_PPM_DOPO")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0);
            let st2 = state.clone();
            let area2 = area.clone();
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(dopo + 200),
                move || {
                    if let Ok(mut st) = st2.try_borrow_mut() {
                        st.full_dirty = true;
                    }
                    area2.queue_render();
                },
            );
        }

        // --- dati dal processo (thread) al ciclo grafico
        {
            let st = state.clone();
            let area_weak = area.downgrade();
            let app_weak = app.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(8), move || {
                let mut had_data = false;
                // Riempite dentro il prestito, usate dopo: la finestra della password
                // non si apre con il `RefCell` dello stato ancora occupato.
                let mut sudo_da_chiedere: Option<(crate::pty::Maniglia, crate::sudo::Bersaglio)> =
                    None;
                {
                    let mut st = st.borrow_mut();
                    if !st.alive {
                        return glib::ControlFlow::Break;
                    }
                    while let Ok(msg) = rx.try_recv() {
                        match msg {
                            Msg::Data(bytes) => {
                                if std::env::var_os("ZIRKIO_LOG_BYTES").is_some() {
                                    use std::io::Write;
                                    if let Ok(mut f) = std::fs::OpenOptions::new()
                                        .create(true)
                                        .append(true)
                                        .open("/tmp/zirkio-bytes.log")
                                    {
                                        let _ = f.write_all(&bytes);
                                    }
                                }
                                // Il prompt di sudo si cerca **prima** del parser:
                                // dopo, le sequenze di escape sono già consumate.
                                if st.rilevatore.alimenta(&bytes) {
                                    tenta_sudo(&st, &app_weak, &tx_sudo);
                                }
                                let ViewState { parser, term, .. } = &mut *st;
                                parser.advance(term, &bytes);
                                had_data = true;
                            }
                            Msg::SudoSenzaPassword(maniglia, chi) => {
                                // La finestra si apre **fuori** da questo prestito:
                                // mostrarla qui dentro significherebbe rientrare in
                                // GTK con il `RefCell` di stato ancora occupato.
                                sudo_da_chiedere = Some((maniglia, chi));
                            }
                            Msg::Exit => {
                                st.exited = true;
                                st.alive = false;
                                // `exit` nella shell: il pannello si chiude da solo
                                // (prima restava lì, morto, con l'ultima schermata).
                                let me = app_weak.clone();
                                glib::idle_add_local_once(move || {
                                    if let Some(a) = me.upgrade() {
                                        a.view_exited(id);
                                    }
                                });
                            }
                        }
                    }
                    if had_data {
                        let events = std::mem::take(&mut st.term.events);
                        if !events.write.is_empty() {
                            st.pty.write(&events.write);
                        }
                        if let Some(text) = events.clipboard {
                            if let Some(a) = area_weak.upgrade() {
                                a.clipboard(&gdk::SELECTION_CLIPBOARD).set_text(&text);
                            }
                        }
                    }
                }
                if let Some((maniglia, chi)) = sudo_da_chiedere {
                    let annulla = st.borrow().tasto.clone();
                    if let Some(app) = app_weak.upgrade() {
                        let ricorda = app.config().sudo.ricorda;
                        crate::sudo::dialogo_password(&app.window, maniglia, annulla, chi, ricorda);
                    }
                }
                if had_data {
                    if let Some(area) = area_weak.upgrade() {
                        area.queue_render();
                    }
                    if let Some(app) = app_weak.upgrade() {
                        app.refresh_titles();
                    }
                }
                glib::ControlFlow::Continue
            });
        }

        // --- tastiera
        {
            let st = state.clone();
            let area_keys = area.clone();
            let shortcuts = on_shortcut.clone();
            let app_tasti = app.clone();
            let tasto_tasti = tasto.clone();
            area.connect_key_press_event(move |area, ev| {
                if std::env::var_os("ZIRKIO_STATS").is_some() {
                    let m = mods_from(ev.state());
                    eprintln!(
                        "[stats] tasto keyval=0x{:x} shift={} ctrl={} alt={} -> {:?}",
                        *ev.keyval(),
                        m.shift,
                        m.ctrl,
                        m.alt,
                        shortcut_for(ev)
                    );
                }
                if let Some(action) = shortcut_for(ev) {
                    let handled = match action {
                        Action::Copy => {
                            copy_selection(&st, &area_keys);
                            true
                        }
                        Action::Paste => {
                            paste_into(&st, &area_keys);
                            true
                        }
                        _ => shortcuts(action),
                    };
                    if handled {
                        return glib::Propagation::Stop;
                    }
                }
                let mut stb = st.borrow_mut();
                // Un tasto dell'utente vale come «ci penso io»: niente password
                // automatica e tentativi azzerati.
                tasto_tasti.store(true, Ordering::Relaxed);
                stb.rilevatore.nota_tasto();
                if stb.term.display_offset != 0 {
                    stb.term.display_offset = 0;
                    stb.term.dirty.mark_all();
                    stb.full_dirty = true;
                }
                // --- `@sh <nome>`: si riconosce la riga **digitata** e, se la sessione
                // esiste, si apre al posto di mandare l'Invio alla shell.  La riga si
                // toglie dallo schermo con Ctrl+U, così il prompt resta pulito.
                let keyval = *ev.keyval();
                stb.riga_digitata = aggiorna_riga(&stb.riga_digitata, keyval, mods_from(ev.state()));
                if e_invio(keyval) {
                    let riga = stb.riga_digitata.trim().to_string();
                    stb.riga_digitata.clear();
                    // --- `@scp <sorgente> <destinazione>`: trasferimento di file o cartelle.
                // Il tipo lo scopre il sistema; `@scp` da solo apre la finestra dei
                // trasferimenti (i due riquadri), come `Ctrl+Maiusc+F`.
                if riga == "@scp" || riga.starts_with("@scp ") {
                    let resto = riga.trim_start_matches("@scp").trim().to_string();
                    // cartella corrente della shell: un percorso relativo va inteso da lì
                    let cwd = stb
                        .pty
                        .pid()
                        .checked_abs()
                        .and_then(crate::pty::cwd_di);
                    stb.pty.write(&[0x15]);
                    drop(stb);
                    if std::env::var_os("ZIRKIO_STATS").is_some() {
                        eprintln!("[stats] @scp: {resto}");
                    }
                    if let Some(a) = app_tasti.upgrade() {
                        let esito = match crate::scp::dividi_argomenti(&resto) {
                            Err(e) => Err(e),
                            Ok(args) if args.is_empty() => {
                                crate::scpwin::apri(&a);
                                Ok(())
                            }
                            Ok(args) if args.len() == 2 => {
                                a.trasferisci(&args[0], &args[1], cwd.as_deref())
                            }
                            Ok(_) => Err("uso: @scp <sorgente> <destinazione>\n\
                                          es. @scp foto.jpg 207:~/Immagini/  ·  \
                                          @scp 207:/var/log ./copia/  ·  \
                                          @scp  (apre la finestra dei trasferimenti)"
                                .to_string()),
                        };
                        if let Err(e) = esito {
                            a.errore_pubblico(&e);
                        }
                    }
                    area.queue_render();
                    return glib::Propagation::Stop;
                }
                if let Some(nome) = riga.strip_prefix("@sh ") {
                        let nome = nome.trim().to_string();
                        let esiste = app_tasti
                            .upgrade()
                            .map(|a| a.ha_sessione(&nome))
                            .unwrap_or(false);
                        if esiste && !nome.is_empty() {
                            if std::env::var_os("ZIRKIO_STATS").is_some() {
                                eprintln!("[stats] @sh: apro la sessione «{nome}»");
                            }
                            stb.pty.write(&[0x15]);
                            drop(stb);
                            if let Some(a) = app_tasti.upgrade() {
                                if let Err(e) = a.apri_sessione(&nome) {
                                    a.errore_pubblico(&e);
                                }
                            }
                            area.queue_render();
                            return glib::Propagation::Stop;
                        }
                    }
                    // --- `@sd <nome>`: spegne una macchina salvata.  La riga si toglie
                    // dallo schermo con Ctrl+U e la conferma si chiede in una finestra:
                    // spegnere una macchina non si fa per sbaglio.
                    if let Some(nome) = riga.strip_prefix("@sd ") {
                        let nome = nome.trim().to_string();
                        if !nome.is_empty() {
                            stb.pty.write(&[0x15]);
                            drop(stb);
                            if let Some(a) = app_tasti.upgrade() {
                                if let Err(e) = a.spegni_sessione(&nome) {
                                    a.errore_pubblico(&e);
                                }
                            }
                            area.queue_render();
                            return glib::Propagation::Stop;
                        }
                    }
                }
                if let Some(bytes) = input::encode_key(Some(*ev.keyval()), mods_from(ev.state()), &stb.term.modes) {
                    stb.pty.write(&bytes);
                }
                drop(stb);
                area.queue_render();
                glib::Propagation::Stop
            });
        }

        // --- mouse
        {
            let st = state.clone();
            let app_for_menu = app.clone();
            area.connect_button_press_event(move |area, ev| {
                area.grab_focus();
                let button = ev.button();
                let mods = mods_from(ev.state());
                if std::env::var_os("ZIRKIO_STATS").is_some() {
                    eprintln!(
                        "[stats] clic tasto={button} pos={:?} fuoco={}",
                        ev.position(),
                        area.has_focus()
                    );
                }
                // Con il mouse "acceso" dall'applicazione (mc, htop, vim…) i clic vanno a lei;
                // Shift resta per selezionare il testo, come in xterm.
                let send_mouse = {
                    let stb = st.borrow();
                    stb.term.modes.mouse != crate::term::MouseMode::Off && !mods.shift
                };
                let Some((col, row)) = cell_at(&st, area, ev.position()) else {
                    return glib::Propagation::Stop;
                };
                match button {
                    1 if send_mouse => {
                        let stb = st.borrow();
                        if let Some(bytes) =
                            input::encode_mouse(&stb.term.modes, MouseKind::Press(0), col, row, mods)
                        {
                            stb.pty.write(&bytes);
                        }
                    }
                    1 => {
                        let (line, col) = {
                            let stb = st.borrow();
                            (stb.term.first_visible_line() + row as i64, col)
                        };
                        let mut open_url = None;
                        if mods.ctrl {
                            open_url = st.borrow().term.url_at(line, col);
                        }
                        {
                            let mut stb = st.borrow_mut();
                            stb.dragging = true;
                            if ev.event_type() == gdk::EventType::DoubleButtonPress {
                                let (a, b) = stb.term.word_at(line, col);
                                stb.sel = Some(Sel { anchor: a, head: b });
                            } else {
                                stb.sel = Some(Sel {
                                    anchor: Point::new(line, col),
                                    head: Point::new(line, col),
                                });
                            }
                            stb.term.dirty.mark_all();
                        }
                        if let Some(url) = open_url {
                            crate::pty::open_url(&url);
                        }
                        area.queue_render();
                    }
                    3 if !send_mouse => {
                        // Menu del tasto destro: `GtkMenu` standard, ancorato al puntatore.
                        if let Some(app) = app_for_menu.upgrade() {
                            if std::env::var_os("ZIRKIO_STATS").is_some() {
                                eprintln!("[stats] menu tasto destro aperto (pannello {id})");
                            }
                            app.apri_menu_pannello(id, ev);
                        }
                    }
                    2 => paste_into(&st, area),
                    _ => {
                        let stb = st.borrow();
                        if let Some(bytes) = input::encode_mouse(
                            &stb.term.modes,
                            MouseKind::Press(button.saturating_sub(1) as u8),
                            col,
                            row,
                            mods,
                        ) {
                            stb.pty.write(&bytes);
                        }
                    }
                }
                glib::Propagation::Stop
            });
        }
        {
            let st = state.clone();
            area.connect_button_release_event(move |area, ev| {
                let button = ev.button();
                let send_mouse = {
                    let stb = st.borrow();
                    stb.term.modes.mouse != crate::term::MouseMode::Off
                        && !mods_from(ev.state()).shift
                };
                let dragging = st.borrow().dragging;
                if send_mouse {
                    if let Some((col, row)) = cell_at(&st, area, ev.position()) {
                        let stb = st.borrow();
                        if let Some(bytes) = input::encode_mouse(
                            &stb.term.modes,
                            MouseKind::Release(button.saturating_sub(1) as u8),
                            col,
                            row,
                            mods_from(ev.state()),
                        ) {
                            stb.pty.write(&bytes);
                        }
                    }
                    return glib::Propagation::Stop;
                }
                if button == 1 && dragging {
                    st.borrow_mut().dragging = false;
                    copy_selection(&st, area);
                } else if let Some((col, row)) = cell_at(&st, area, ev.position()) {
                    let stb = st.borrow();
                    if let Some(bytes) = input::encode_mouse(
                        &stb.term.modes,
                        MouseKind::Release(button as u8),
                        col,
                        row,
                        mods_from(ev.state()),
                    ) {
                        stb.pty.write(&bytes);
                    }
                }
                glib::Propagation::Stop
            });
        }
        {
            let st = state.clone();
            area.connect_motion_notify_event(move |area, ev| {
                let send_mouse = {
                    let stb = st.borrow();
                    stb.term.modes.mouse == crate::term::MouseMode::Drag
                        || stb.term.modes.mouse == crate::term::MouseMode::Motion
                };
                if send_mouse && !st.borrow().dragging {
                    if let Some((col, row)) = cell_at(&st, area, ev.position()) {
                        let stb = st.borrow();
                        if let Some(bytes) =
                            input::encode_mouse(&stb.term.modes, MouseKind::Motion(0), col, row, mods_from(ev.state()))
                        {
                            stb.pty.write(&bytes);
                        }
                    }
                    return glib::Propagation::Stop;
                }
                if !st.borrow().dragging {
                    return glib::Propagation::Proceed;
                }
                if let Some((col, row)) = cell_at(&st, area, ev.position()) {
                    {
                        let mut stb = st.borrow_mut();
                        let line = stb.term.first_visible_line() + row as i64;
                        if let Some(sel) = stb.sel.as_mut() {
                            sel.head = Point::new(line, col);
                        }
                        stb.term.dirty.mark_all();
                    }
                    area.queue_render();
                }
                glib::Propagation::Stop
            });
        }
        {
            let st = state.clone();
            area.connect_scroll_event(move |area, ev| {
                // `direction()` non ha ambiguità di segno (i delta su Wayland sono invertiti
                // rispetto a X11: era per questo che la rotella girava al contrario).
                let lines = match ev.direction() {
                    gdk::ScrollDirection::Up => 3,
                    gdk::ScrollDirection::Down => -3,
                    gdk::ScrollDirection::Smooth => {
                        let (_, dy) = ev.scroll_deltas().unwrap_or((0.0, 0.0));
                        if dy < 0.0 { 3 } else if dy > 0.0 { -3 } else { 0 }
                    }
                    _ => 0,
                };
                if lines == 0 {
                    return glib::Propagation::Stop;
                }
                // ⚠️ `cell_at` prende in prestito lo stato: va chiamata **prima** del
                // `borrow_mut`, altrimenti si prende un secondo prestito e l'app va in
                // panico (`RefCell already mutably borrowed`).  Succedeva con la rotella
                // su una tendina di mc: mc accende il mouse e lo scroll entra nel ramo
                // "manda la rotella al processo", che chiamava `cell_at` dentro il mut.
                let cella = cell_at(&st, area, ev.position());
                {
                    let mut stb = st.borrow_mut();
                    let mouse_mode = stb.term.modes.mouse != crate::term::MouseMode::Off;
                    if mouse_mode && stb.term.modes.alt_scroll {
                        if let Some((col, row)) = cella {
                            let kind = if lines > 0 { MouseKind::WheelUp } else { MouseKind::WheelDown };
                            if let Some(bytes) =
                                input::encode_mouse(&stb.term.modes, kind, col, row, mods_from(ev.state()))
                            {
                                stb.pty.write(&bytes);
                            }
                        }
                    } else {
                        let max = stb.term.scrollback.len() as i32;
                        let cur = stb.term.display_offset as i32;
                        let next = (cur + lines * 3).clamp(0, max);
                        if next != cur {
                            stb.term.display_offset = next as usize;
                            stb.term.dirty.mark_all();
                            stb.full_dirty = true;
                        }
                    }
                }
                area.queue_render();
                glib::Propagation::Stop
            });
        }

        // Chi prende il fuoco diventa il pannello attivo: prima restava quello di prima,
        // quindi copia/incolla, dividi e chiudi agivano sull'altro pannello.
        {
            let me = app.clone();
            area.connect_focus_in_event(move |_, _| {
                if let Some(a) = me.upgrade() {
                    a.set_active(id);
                }
                glib::Propagation::Proceed
            });
        }
        {
            let me = app.clone();
            area.connect_button_press_event(move |_, _| {
                if let Some(a) = me.upgrade() {
                    a.set_active(id);
                }
                glib::Propagation::Proceed
            });
        }

        let label = gtk::Label::new(Some("Terminale"));
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(32);
        // Larghezza minima: senza questa la scheda si stringe fino a "..."
        label.set_width_chars(14);
        label.set_xalign(0.0);
        let tab_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        // Senza `expand` la scheda prende la larghezza del testo (con expand resta "...").
        tab_box.pack_start(&label, false, false, 0);
        tab_box.show_all();

        Ok(Rc::new(View { id, area, state, label, tab_box, titolo_fisso }))
    }

    /// Chiude il pannello: ferma il ciclo che legge il pty e **tiene vivo lo stato**.
    ///
    /// Senza questo, chiudendo una scheda la shell restava viva (e non salvava mai la
    /// cronologia) perché il `timeout_add_local` del pannello tratteneva lo stato: il
    /// `Drop` della pty non veniva mai eseguito.  Con `alive = false` il ciclo esce al
    /// giro dopo, l'ultimo riferimento cade e la pty chiude il master → SIGHUP alla shell
    /// → zsh/bash scrivono il file di cronologia.
    pub fn chiudi(&self) {
        if let Ok(mut st) = self.state.try_borrow_mut() {
            st.alive = false;
            // SIGHUP alla shell: esce e **salva la cronologia** (prima la si ammazzava
            // con SIGKILL, o addirittura restava viva perché il pannello non veniva mai
            // liberato).
            st.pty.termina();
        }
    }

    /// Testo della selezione, se c'è.
    pub fn selection(&self) -> Option<String> {
        let st = self.state.borrow();
        let sel = st.sel.as_ref()?;
        let text = st.term.selection_text(sel.anchor, sel.head);
        if text.is_empty() { None } else { Some(text) }
    }

    pub fn copy(&self) {
        if let Some(text) = self.selection() {
            self.area.clipboard(&gdk::SELECTION_CLIPBOARD).set_text(&text);
        }
    }

    pub fn paste(&self) {
        if let Some(text) = self.area.clipboard(&gdk::SELECTION_CLIPBOARD).wait_for_text() {
            let st = self.state.borrow();
            let bytes = input::encode_paste(&st.term.modes, &text);
            st.pty.write(&bytes);
        }
        self.area.queue_render();
    }

    pub fn select_all(&self) {
        {
            let mut st = self.state.borrow_mut();
            let (a, b) = st.term.select_all_points();
            st.sel = Some(Sel { anchor: a, head: b });
            st.term.dirty.mark_all();
        }
        self.area.queue_render();
    }

    pub fn scroll_page(&self, up: bool) {
        {
            let mut st = self.state.borrow_mut();
            let step = st.term.rows as i32 * if up { 3 } else { -3 };
            let max = st.term.scrollback.len() as i32;
            let next = (st.term.display_offset as i32 + step).clamp(0, max);
            st.term.display_offset = next as usize;
            st.term.dirty.mark_all();
            st.full_dirty = true;
        }
        self.area.queue_render();
    }

    pub fn scroll_end(&self, bottom: bool) {
        {
            let mut st = self.state.borrow_mut();
            st.term.display_offset = if bottom { 0 } else { st.term.scrollback.len() };
            st.term.dirty.mark_all();
            st.full_dirty = true;
        }
        self.area.queue_render();
    }

    pub fn retint(&self, old: &crate::config::Theme, new: &crate::config::Theme) {
        {
            let mut st = self.state.borrow_mut();
            let old_pal = old.palette256();
            let new_pal = new.palette256();
            st.term.retint(old.fg, old.bg, &old_pal, new.fg, new.bg, &new_pal);
            st.full_dirty = true;
        }
        self.area.queue_render();
    }

    pub fn set_fonts(&self, fonts: Rc<FontSet>) {
        {
            let mut st = self.state.borrow_mut();
            // L'atlante (texture dei glifi) si rifà **nel ciclo di disegno**, dove il
            // contesto OpenGL è corrente: qui non si tocca il renderer.
            st.fonts = fonts.clone();
            st.pending_fonts = Some(fonts);
            st.full_dirty = true;
        }
        self.area.queue_resize();
        self.area.queue_render();
    }

    /// Cambia il margine interno (in pixel logici) e ridisegna tutto.
    pub fn set_padding(&self, padding: u32) {
        let scale = self.area.scale_factor() as f32;
        {
            let mut st = self.state.borrow_mut();
            st.padding = padding as f32 * scale;
            st.full_dirty = true;
        }
        self.area.queue_resize();
        self.area.queue_render();
    }

    /// Righe di cronologia conservate.
    pub fn set_scrollback(&self, righe: usize) {
        let mut st = self.state.borrow_mut();
        st.term.scrollback_max = righe;
        while st.term.scrollback.len() > righe {
            st.term.scrollback.pop_front();
        }
        st.full_dirty = true;
        self.area.queue_render();
    }

    /// Forma del cursore.
    pub fn set_cursor_style(&self, style: crate::term::CursorStyle) {
        let mut st = self.state.borrow_mut();
        st.term.cursor.style = style;
        st.full_dirty = true;
        self.area.queue_render();
    }

    pub fn title(&self) -> String {
        self.state.borrow().term.label()
    }
}

/// Il prompt di `sudo` è arrivato: se la funzione è accesa, manda la password.
///
/// Qui si decide soltanto **se** vale la pena: i controlli veri (`/proc`, eco del
/// terminale, portachiavi) possono aspettare decine di millisecondi — e il
/// portachiavi anche di più — quindi si fanno in un thread, mai nel ciclo grafico.
fn tenta_sudo(st: &ViewState, app: &std::rc::Weak<crate::app::App>, tx: &Sender<Msg>) {
    // La configurazione si legge qui, nel ciclo grafico: `App` non è `Send` e
    // dentro il thread non si potrebbe toccare.
    let Some(a) = app.upgrade() else {
        return;
    };
    let cfg = a.config();
    // La funzione è facoltativa e spenta di default: senza il consenso dell'utente
    // non si scrive niente, nemmeno la password che sarebbe già in memoria.
    if !cfg.sudo.inserimento {
        return;
    }
    let Some(maniglia) = st.pty.maniglia() else {
        return;
    };
    // Si riparte da «nessun tasto»: il segnale serve a fermare l'invio se l'utente
    // comincia a digitare mentre aspettiamo che l'eco si spenga.  Senza questo
    // azzeramento, un tasto premuto molto prima bloccherebbe per sempre la funzione.
    st.tasto.store(false, Ordering::Relaxed);
    let annulla = st.tasto.clone();
    let ricorda = cfg.sudo.ricorda;
    let tx = tx.clone();
    std::thread::spawn(move || {
        if annulla.load(Ordering::Relaxed) {
            return;
        }
        // Chi chiede: sudo qui, oppure l'ssh di una sessione remota (e in quel caso
        // serve la password di **quella** macchina).  Il prompt scritto a mano da
        // un altro processo non passa questo controllo.
        let Some(chi) = crate::sudo::bersaglio(&maniglia) else {
            return;
        };
        // 1. quella che c'è in memoria per questa macchina
        if let Some(pw) = crate::sudo::password(&chi.chiave) {
            crate::sudo::invia(maniglia, pw, &annulla, &chi.chiave);
            return;
        }
        // 2. quella del portachiavi, ma solo se l'utente ha scelto di ricordarla
        if ricorda {
            if let Some(pw) = crate::sudo::dal_portachiavi(&chi.chiave) {
                crate::sudo::imposta(&chi.chiave, &pw);
                crate::sudo::invia(maniglia, pw, &annulla, &chi.chiave);
                return;
            }
        }
        // 3. non ce n'è nessuna: si chiede al ciclo grafico (che è l'unico a poter
        //    aprire una finestra) rimandandogli la maniglia e la macchina
        let _ = tx.send(Msg::SudoSenzaPassword(maniglia, chi));
    });
}

impl ViewState {
    /// Ricalcola colonne e righe dalla misura dell'area e ridimensiona il processo.
    fn sync_grid(&mut self, w: i32, h: i32) {
        // Un carattere nuovo si applica QUI (siamo dentro `connect_render`, col contesto
        // OpenGL corrente): rifare l'atlante fuori da qui non ha effetto e lascia le
        // celle con i glifi vecchi, cioè testo illeggibile (segnalato dall'utente il
        // 24/10/2026 scegliendo «carattere più grande»).
        if let Some(fs) = self.pending_fonts.take() {
            match self.renderer.as_mut() {
                Some(r) => {
                    r.set_fonts(fs);
                    self.full_dirty = true;
                }
                None => {
                    self.pending_fonts = Some(fs);
                    return;
                }
            }
        }
        let Some(r) = self.renderer.as_ref() else { return };
        let cw = r.cell_w().max(1.0);
        let chh = r.cell_h().max(1.0);
        let cols = (((w as f32 - self.padding * 2.0) / cw).floor() as usize).max(2);
        let rows = (((h as f32 - self.padding * 2.0) / chh).floor() as usize).max(2);
        if cols != self.cols || rows != self.rows {
            self.cols = cols;
            self.rows = rows;
            self.term.resize(cols, rows);
            self.pty.resize(cols, rows);
            self.full_dirty = true;
        }
    }

    /// Disegna la griglia (solo righe sporche) più il cursore.
    ///
    /// I buffer di lavoro (`scratch`, `overlay`, `riga_vuota`) vivono nello stato e
    /// vengono presi e rimessi: nessuna allocazione per fotogramma.
    fn paint(&mut self, w: i32, h: i32) {
        let self_bg = self.term.default_bg;
        let sel_color = self.term.palette[8].mix(self.term.default_fg, 0.15);
        let mut scratch = std::mem::take(&mut self.scratch);
        let mut overlay = std::mem::take(&mut self.overlay);
        let mut riga_vuota = std::mem::take(&mut self.riga_vuota);
        {
            let Some(renderer) = self.renderer.as_mut() else {
                self.scratch = scratch;
                self.overlay = overlay;
                self.riga_vuota = riga_vuota;
                return;
            };
            renderer.set_size(w, h);
            let realloc = renderer.reserve_grid(self.cols, self.rows);
            let full = self.full_dirty || realloc;
            let cw = renderer.cell_w();

            let (crow, ccol) = (self.term.cursor.row, self.term.cursor.col);
            if (crow, ccol) != self.last_cursor {
                self.term.dirty.mark(self.last_cursor.0);
                self.term.dirty.mark(crow);
                self.last_cursor = (crow, ccol);
            }
            let show_cursor = self.term.cursor.visible && self.term.display_offset == 0;
            let first = self.term.first_visible_line();
            let blank = self.term.blank_cell();
            if riga_vuota.len() != self.cols {
                riga_vuota = vec![blank; self.cols];
            }
            let x0 = self.padding;

            // Si disegna il blocco continuo che va dalla prima all'ultima riga
            // sporca: una sola chiamata di carico invece di una per riga.
            let (da, a) = if full {
                (0, self.rows.saturating_sub(1))
            } else {
                match (0..self.rows).find(|r| self.term.dirty.is_dirty(*r)) {
                    Some(prima) => (
                        prima,
                        (0..self.rows).rev().find(|r| self.term.dirty.is_dirty(*r)).unwrap_or(prima),
                    ),
                    None => (0, 0),
                }
            };
            if full || self.term.dirty.is_dirty(da) {
                scratch.clear();
                let per_cella = crate::render::INST_PER_CELL;
                let serve = (a + 1 - da) * self.cols * per_cella;
                if scratch.capacity() < serve {
                    scratch.reserve(serve);
                }
                for row in da..=a {
                    let line = first + row as i64;
                    let sel = self.sel.as_ref().and_then(|s| sel_range(s, line, self.cols));
                    let cursor = if show_cursor && crow == row { Some(ccol) } else { None };
                    let cells: &[crate::term::Cell] =
                        self.term.line_cells(line).unwrap_or(&riga_vuota[..]);
                    let y0 = self.padding + row as f32 * renderer.cell_h();
                    renderer.row_instances(
                        cells, x0, y0, sel, cursor, sel_color, self_bg, &mut scratch,
                    );
                }
                renderer.upload_span(da, &scratch);
                if std::env::var_os("ZIRKIO_DEBUG_RENDER").is_some() {
                    use std::sync::atomic::{AtomicU32, Ordering};
                    static N: AtomicU32 = AtomicU32::new(0);
                    let n = N.fetch_add(1, Ordering::Relaxed);
                    if n < 5 {
                        let (cap, cnt) = renderer.debug_counts();
                        eprintln!(
                            "[debug] frame {n}: {w}x{h} celle {}x{} full={full} righe {da}..{a} istanze={} cap={cap} count={cnt} cella={}x{}",
                            self.cols, self.rows, scratch.len(), renderer.cell_w(), renderer.cell_h()
                        );
                        for (i, inst) in scratch.iter().enumerate().take(8) {
                            eprintln!(
                                "[debug]   #{i} x={:.1} y={:.1} w={:.1} h={:.1} rgba={:.2},{:.2},{:.2},{:.2} uv={:.4},{:.4}->{:.4},{:.4}",
                                inst.x, inst.y, inst.w, inst.h, inst.r, inst.g, inst.b, inst.a, inst.u0, inst.v0, inst.u1, inst.v1
                            );
                        }
                    }
                }
            }
            self.term.dirty.take();
            self.full_dirty = false;

            // Cursore a barra / sottolineatura (il blocco è già invertito nella cella).
            if show_cursor {
                let style = self.term.cursor.style;
                if style != crate::term::CursorStyle::Block {
                    let x = self.padding + ccol as f32 * cw;
                    let y = self.padding + crow as f32 * renderer.cell_h();
                    let chh = renderer.cell_h();
                    let col = self.term.palette[7];
                    if style == crate::term::CursorStyle::Beam {
                        renderer.ui_rect(&mut overlay, x, y + 1.0, 2.0, chh - 2.0, col);
                    } else {
                        renderer.ui_rect(&mut overlay, x, y + chh - 2.0, cw, 2.0, col);
                    }
                }
            }
            renderer.draw(self_bg, &overlay);
        }
        self.scratch = scratch;
        self.overlay = overlay;
        self.riga_vuota = riga_vuota;
    }
}

fn sel_range(sel: &Sel, line: i64, cols: usize) -> Option<(usize, usize)> {
    let (a, b) = if (sel.anchor.line, sel.anchor.col) <= (sel.head.line, sel.head.col) {
        (sel.anchor, sel.head)
    } else {
        (sel.head, sel.anchor)
    };
    if line < a.line || line > b.line {
        return None;
    }
    let x0 = if line == a.line { a.col } else { 0 };
    let x1 = if line == b.line { b.col } else { cols.saturating_sub(1) };
    if x0 > x1 {
        return None;
    }
    Some((x0.min(cols - 1), x1.min(cols - 1)))
}

/// Tiene traccia dei caratteri digitati nella riga corrente (per riconoscere `@sh nome`).
///
/// È una **euristica**: si azzera a ogni tasto "speciale" (frecce, Tab, F1…), perché da
/// lì in poi non si sa più dove sia il cursore nella riga della shell.
fn aggiorna_riga(riga: &str, keyval: u32, mods: Mods) -> String {
    if mods.ctrl || mods.alt {
        return String::new();
    }
    match keyval {
        0xff0d | 0xff8d => riga.to_string(), // Invio: la riga serve al controllo
        0xff08 | 0x0008 | 0x007f => {
            let mut s = riga.to_string();
            s.pop();
            s
        }
        0x20..=0x7e => {
            let mut s = riga.to_string();
            if let Some(c) = char::from_u32(keyval) {
                s.push(c);
            }
            s
        }
        _ => String::new(),
    }
}

/// Vero per Invio (tastiera principale e tastierino).
fn e_invio(keyval: u32) -> bool {
    keyval == 0xff0d || keyval == 0xff8d
}

/// Cella (colonna, riga) sotto il puntatore.
fn cell_at(state: &Rc<RefCell<ViewState>>, area: &gtk::GLArea, pos: (f64, f64)) -> Option<(usize, usize)> {
    let st = state.borrow();
    let r = st.renderer.as_ref()?;
    let scale = area.scale_factor() as f32;
    let (x, y) = (pos.0 as f32 * scale, pos.1 as f32 * scale);
    let cw = r.cell_w().max(1.0);
    let chh = r.cell_h().max(1.0);
    let col = ((x - st.padding) / cw).floor();
    let row = ((y - st.padding) / chh).floor();
    if col < 0.0 || row < 0.0 {
        return None;
    }
    let (col, row) = (col as usize, row as usize);
    if col >= st.cols || row >= st.rows {
        return None;
    }
    Some((col, row))
}

fn mods_from(s: gdk::ModifierType) -> Mods {
    Mods {
        shift: s.contains(gdk::ModifierType::SHIFT_MASK),
        ctrl: s.contains(gdk::ModifierType::CONTROL_MASK),
        alt: s.contains(gdk::ModifierType::MOD1_MASK),
        sup: s.contains(gdk::ModifierType::SUPER_MASK) || s.contains(gdk::ModifierType::MOD4_MASK),
    }
}

/// Scorciatoie dell'applicazione che hanno la precedenza sul processo.
pub fn shortcut_for(ev: &gdk::EventKey) -> Option<Action> {
    use crate::term::input::keys as k;
    let key = *ev.keyval();
    let m = mods_from(ev.state());
    if m.ctrl && m.shift {
        return match key {
            x if x == 't' as u32 => Some(Action::NewTab),
            x if x == 'T' as u32 => Some(Action::NewTab),
            x if x == 'w' as u32 => Some(Action::ClosePane),
            x if x == 'W' as u32 => Some(Action::ClosePane),
            x if x == 'd' as u32 => Some(Action::SplitRight),
            x if x == 'D' as u32 => Some(Action::SplitRight),
            x if x == 'e' as u32 => Some(Action::SplitDown),
            x if x == 'E' as u32 => Some(Action::SplitDown),
            x if x == 'c' as u32 => Some(Action::Copy),
            x if x == 'C' as u32 => Some(Action::Copy),
            x if x == 'v' as u32 => Some(Action::Paste),
            x if x == 'V' as u32 => Some(Action::Paste),
            x if x == 'a' as u32 => Some(Action::SelectAll),
            x if x == 'A' as u32 => Some(Action::SelectAll),
            x if x == 's' as u32 => Some(Action::SessioniSsh),
            x if x == 'S' as u32 => Some(Action::SessioniSsh),
            x if x == 'f' as u32 => Some(Action::Trasferimenti),
            x if x == 'F' as u32 => Some(Action::Trasferimenti),
            x if x == 'q' as u32 => Some(Action::Quit),
            x if x == 'Q' as u32 => Some(Action::Quit),
            0x002b | 0x003d | 0xffab => Some(Action::FontBigger),
            0x002d | 0x005f | 0xffad => Some(Action::FontSmaller),
            0x0030 | 0x0029 => Some(Action::FontReset),
            k::RIGHT => Some(Action::FocusNext),
            k::LEFT => Some(Action::FocusPrev),
            k::UP => Some(Action::FocusPrev),
            k::DOWN => Some(Action::FocusNext),
            _ => None,
        };
    }
    if m.ctrl && !m.shift {
        match key {
            0x002c => return Some(Action::Prefs), // Ctrl+virgola: configurazione
            k::PAGE_UP => return Some(Action::PrevTab),
            k::PAGE_DOWN => return Some(Action::NextTab),
            k::TAB => return Some(Action::NextTab),
            k::HOME => return Some(Action::ScrollTop),
            k::END => return Some(Action::ScrollBottom),
            _ => {}
        }
    }
    // Maiusc+PgSu/PgGiù = cronologia (come il resto dei terminali).  **PgSu/PgGiù senza
    // modificatori vanno al processo**: prima li intercettavamo sempre e dentro `mc`,
    // `less`, `vim`… le pagine non scorrevano (segnalato dall'utente il 24/10).
    if m.shift && !m.ctrl && !m.alt {
        match key {
            k::PAGE_UP => return Some(Action::ScrollPageUp),
            k::PAGE_DOWN => return Some(Action::ScrollPageDown),
            _ => {}
        }
    }
    if !m.ctrl && !m.alt && !m.shift {
        match key {
            k::F1 => return Some(Action::Help),
            k::F2 => return Some(Action::ThemeNext),
            _ => {}
        }
    }
    None
}

fn copy_selection(state: &Rc<RefCell<ViewState>>, area: &gtk::GLArea) {
    let text = {
        let st = state.borrow();
        st.sel
            .as_ref()
            .map(|s| st.term.selection_text(s.anchor, s.head))
            .filter(|t| !t.is_empty())
    };
    if let Some(text) = text {
        area.clipboard(&gdk::SELECTION_CLIPBOARD).set_text(&text);
    }
}

fn paste_into(state: &Rc<RefCell<ViewState>>, area: &gtk::GLArea) {
    if let Some(text) = area.clipboard(&gdk::SELECTION_CLIPBOARD).wait_for_text() {
        let st = state.borrow();
        let bytes = input::encode_paste(&st.term.modes, &text);
        st.pty.write(&bytes);
    }
    area.queue_render();
}
