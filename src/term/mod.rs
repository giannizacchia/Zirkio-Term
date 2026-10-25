//! Griglia del terminale: celle, schermo, scrollback, cursore, selezione.
//!
//! La rappresentazione è una riga per `Vec<Cell>` (non una griglia piatta):
//! scorrere significa spostare un puntatore invece di copiare mezza schermata,
//! e le righe mandate nello scrollback ci finiscono senza copia.

use std::collections::VecDeque;

use unicode_width::UnicodeWidthChar;

pub mod input;
pub mod performer;

/// Spaziatura delle tabulazioni (come xterm).
pub const TAB_SPACES: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb(r, g, b)
    }

    /// `#rrggbb` oppure `rrggbb`.
    pub fn from_hex(s: &str) -> Option<Rgb> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 || !s.is_ascii() {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let f = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8;
        Rgb(f(self.0, other.0), f(self.1, other.1), f(self.2, other.2))
    }

    /// Colore in virgola mobile, come lo vuole il vertex buffer.
    pub fn rgba(self) -> [f32; 4] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
            1.0,
        ]
    }
}

pub mod flags {
    pub const BOLD: u16 = 1 << 0;
    pub const ITALIC: u16 = 1 << 1;
    pub const UNDERLINE: u16 = 1 << 2;
    pub const STRIKE: u16 = 1 << 3;
    pub const DIM: u16 = 1 << 4;
    pub const INVERSE: u16 = 1 << 5;
    pub const HIDDEN: u16 = 1 << 6;
    /// Continuazione di un carattere largo (occupa la cella ma non si disegna).
    pub const WIDE_SPACER: u16 = 1 << 7;
    /// La riga continua sulla successiva (per copiare/incollare senza a capo finti).
    pub const WRAPPED: u16 = 1 << 8;
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub flags: u16,
}

impl Cell {
    pub const fn blank(fg: Rgb, bg: Rgb) -> Cell {
        Cell { ch: ' ', fg, bg, flags: 0 }
    }

    pub fn is_spacer(&self) -> bool {
        self.flags & flags::WIDE_SPACER != 0
    }

    /// Cella vuota (spazio con gli attributi di base): si può saltare.
    fn is_blank(&self, bg: Rgb) -> bool {
        self.ch == ' ' && self.bg == bg && self.flags & !flags::WRAPPED == 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CursorStyle {
    #[default]
    Block,
    Beam,
    Underline,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub style: CursorStyle,
    pub visible: bool,
    /// Lampeggio: spento di default, così da fermo il terminale non disegna nulla.
    pub blink: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MouseMode {
    #[default]
    Off,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, Debug)]
pub struct Modes {
    pub wrap: bool,
    pub origin: bool,
    pub insert: bool,
    pub app_cursor: bool,
    pub app_keypad: bool,
    pub bracketed_paste: bool,
    pub mouse: MouseMode,
    pub sgr_mouse: bool,
    pub utf8_mouse: bool,
    pub focus_events: bool,
    pub alt_scroll: bool,
}

impl Default for Modes {
    fn default() -> Self {
        Modes {
            wrap: true,
            origin: false,
            insert: false,
            app_cursor: false,
            app_keypad: false,
            bracketed_paste: false,
            mouse: MouseMode::Off,
            sgr_mouse: false,
            utf8_mouse: false,
            focus_events: false,
            alt_scroll: true,
        }
    }
}

/// Righe sporche: si ridisegna solo quello che è cambiato.
pub struct Dirty {
    pub full: bool,
    rows: Vec<bool>,
    any: bool,
}

impl Dirty {
    fn new(rows: usize) -> Dirty {
        Dirty { full: true, rows: vec![false; rows], any: true }
    }

    fn resize(&mut self, rows: usize) {
        self.rows.clear();
        self.rows.resize(rows, true);
        self.full = true;
        self.any = true;
    }

    #[inline]
    pub fn mark(&mut self, row: usize) {
        if !self.full && row < self.rows.len() {
            self.rows[row] = true;
        }
        self.any = true;
    }

    #[inline]
    pub fn mark_all(&mut self) {
        self.full = true;
        self.any = true;
        self.rows.iter_mut().for_each(|r| *r = true);
    }

    #[inline]
    pub fn is_dirty(&self, row: usize) -> bool {
        self.full || self.rows.get(row).copied().unwrap_or(true)
    }

    /// Consuma lo stato: dice se c'era qualcosa da disegnare e azzera i segni.
    pub fn take(&mut self) -> bool {
        let had = self.any || self.full;
        self.full = false;
        self.any = false;
        self.rows.iter_mut().for_each(|r| *r = false);
        had
    }

    pub fn peek(&self) -> bool {
        self.any || self.full
    }
}

/// Posizione assoluta in una riga, contando anche lo scrollback.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub line: i64,
    pub col: usize,
}

impl Point {
    pub fn new(line: i64, col: usize) -> Point {
        Point { line, col }
    }

    fn order(self, other: Point) -> (Point, Point) {
        if (self.line, self.col) <= (other.line, other.col) {
            (self, other)
        } else {
            (other, self)
        }
    }
}

/// Cosa si vuole dal terminale dopo uno scarico di byte.
#[derive(Default, Debug)]
pub struct Events {
    /// Byte da riscrivere al processo (risposte a query, DSR, DA…).
    pub write: Vec<u8>,
    /// Testo da mettere negli appunti (OSC 52).
    pub clipboard: Option<String>,
    pub bell: bool,
    pub title: Option<String>,
    pub exited: bool,
}

pub struct Terminal {
    pub cols: usize,
    pub rows: usize,
    pub lines: Vec<Vec<Cell>>,
    pub scrollback: VecDeque<Vec<Cell>>,
    pub scrollback_max: usize,
    pub cursor: Cursor,
    saved_cursor: (usize, usize),
    pub scroll_top: usize,
    pub scroll_bottom: usize,
    pub tabstops: Vec<bool>,
    pub modes: Modes,
    pub pen: Cell,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub palette: [Rgb; 256],
    pub charset_g: [bool; 2],
    pub charset_active: usize,
    pub wrap_next: bool,
    pub dirty: Dirty,
    /// 0 = fondo; >0 = si sta guardando lo scrollback.
    pub display_offset: usize,
    /// Griglia della schermata principale, messa da parte mentre c'è quella alternata.
    saved_lines: Option<Vec<Vec<Cell>>>,
    pub title: String,
    pub events: Events,
    /// Fine dell'ultima stampa: per il "ripeti carattere" (CSI b).
    last_print: char,
    /// Contatore di generazione: cresce a ogni modifica della griglia.
    pub epoch: u64,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize, scrollback_max: usize, palette: [Rgb; 256], fg: Rgb, bg: Rgb) -> Terminal {
        let cols = cols.max(2);
        let rows = rows.max(2);
        let mut t = Terminal {
            cols,
            rows,
            lines: Vec::new(),
            scrollback: VecDeque::new(),
            scrollback_max,
            cursor: Cursor { row: 0, col: 0, style: CursorStyle::Block, visible: true, blink: false },
            saved_cursor: (0, 0),
            scroll_top: 0,
            scroll_bottom: rows - 1,
            tabstops: Vec::new(),
            modes: Modes::default(),
            pen: Cell::blank(fg, bg),
            default_fg: fg,
            default_bg: bg,
            palette,
            charset_g: [false, false],
            charset_active: 0,
            wrap_next: false,
            dirty: Dirty::new(rows),
            display_offset: 0,
            saved_lines: None,
            title: String::new(),
            events: Events::default(),
            last_print: ' ',
            epoch: 1,
        };
        t.build_tabstops();
        t.reset_grid();
        t
    }

    fn build_tabstops(&mut self) {
        self.tabstops.clear();
        self.tabstops.resize(self.cols, false);
        let mut c = 0;
        while c < self.cols {
            self.tabstops[c] = true;
            c += TAB_SPACES;
        }
    }

    fn reset_grid(&mut self) {
        let blank = Cell::blank(self.default_fg, self.default_bg);
        self.lines.clear();
        for _ in 0..self.rows {
            self.lines.push(vec![blank; self.cols]);
        }
        self.dirty.mark_all();
        self.epoch += 1;
    }

    pub fn blank_cell(&self) -> Cell {
        Cell::blank(self.default_fg, self.default_bg)
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2);
        let rows = rows.max(2);
        if cols == self.cols && rows == self.rows {
            return;
        }
        let blank = self.blank_cell();
        for line in &mut self.lines {
            line.resize(cols, blank);
            line.truncate(cols);
        }
        // Righe in più: si tolgono dal basso, in meno: si aggiungono in fondo.
        self.lines.resize_with(rows, || blank_row(cols, blank));
        self.lines.truncate(rows);
        self.cols = cols;
        self.rows = rows;
        self.cursor.row = self.cursor.row.min(rows - 1);
        self.cursor.col = self.cursor.col.min(cols - 1);
        self.scroll_top = 0;
        self.scroll_bottom = rows - 1;
        self.wrap_next = false;
        self.build_tabstops_preserve();
        self.dirty.resize(rows);
        self.dirty.mark_all();
        self.epoch += 1;
    }

    fn build_tabstops_preserve(&mut self) {
        let old = std::mem::take(&mut self.tabstops);
        self.tabstops.resize(self.cols, false);
        for (i, stop) in old.iter().enumerate().take(self.cols) {
            self.tabstops[i] = *stop;
        }
        if self.tabstops.iter().all(|s| !*s) {
            self.build_tabstops();
        }
    }

    // ---------------------------------------------------------------- griglia

    #[inline]
    pub fn cell(&self, row: usize, col: usize) -> Cell {
        self.lines
            .get(row)
            .and_then(|l| l.get(col))
            .copied()
            .unwrap_or(Cell::blank(self.default_fg, self.default_bg))
    }

    #[inline]
    fn put(&mut self, row: usize, col: usize, cell: Cell) {
        if row < self.rows && col < self.cols {
            self.lines[row][col] = cell;
            self.dirty.mark(row);
        }
    }

    fn clear_row(&mut self, row: usize) {
        let blank = self.pen_bg_cell();
        if row < self.rows {
            for cell in &mut self.lines[row] {
                *cell = blank;
            }
            self.dirty.mark(row);
        }
    }

    /// Cella vuota con lo sfondo corrente (per cancellazioni con SGR impostato).
    fn pen_bg_cell(&self) -> Cell {
        Cell::blank(self.default_fg, self.pen.bg)
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let top = top.min(self.rows - 1);
        let bottom = bottom.min(self.rows - 1);
        if top < bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
            self.goto_home();
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        let top = self.scroll_top;
        let bottom = self.scroll_bottom;
        let n = n.min(bottom - top + 1);
        if n == 0 {
            return;
        }
        let blank = self.pen_bg_cell();
        for _ in 0..n {
            let line = self.lines.remove(top);
            self.lines.insert(bottom, blank_row(self.cols, blank));
            if top == 0 && self.scrollback_max > 0 {
                self.scrollback.push_back(line);
                while self.scrollback.len() > self.scrollback_max {
                    self.scrollback.pop_front();
                }
                if self.display_offset > 0 {
                    self.display_offset = self.display_offset.min(self.scrollback.len());
                }
            }
        }
        for row in top..=bottom {
            self.dirty.mark(row);
        }
        self.epoch += 1;
    }

    pub fn scroll_down(&mut self, n: usize) {
        let top = self.scroll_top;
        let bottom = self.scroll_bottom;
        let n = n.min(bottom - top + 1);
        let blank = self.pen_bg_cell();
        for _ in 0..n {
            self.lines.remove(bottom);
            self.lines.insert(top, blank_row(self.cols, blank));
        }
        for row in top..=bottom {
            self.dirty.mark(row);
        }
        self.epoch += 1;
    }

    // ---------------------------------------------------------------- cursore

    pub fn goto(&mut self, row: usize, col: usize) {
        let (mut row, mut col) = (row, col);
        if self.modes.origin {
            row += self.scroll_top;
            row = row.min(self.scroll_bottom);
        } else {
            row = row.min(self.rows - 1);
        }
        col = col.min(self.cols - 1);
        self.cursor.row = row;
        self.cursor.col = col;
        self.wrap_next = false;
    }

    pub fn goto_home(&mut self) {
        let row = if self.modes.origin { self.scroll_top } else { 0 };
        self.cursor.row = row;
        self.cursor.col = 0;
        self.wrap_next = false;
    }

    pub fn move_up(&mut self, n: usize) {
        let top = if self.cursor.row >= self.scroll_top { self.scroll_top } else { 0 };
        self.cursor.row = self.cursor.row.saturating_sub(n).max(top);
        self.wrap_next = false;
    }

    pub fn move_down(&mut self, n: usize) {
        let bottom = if self.cursor.row <= self.scroll_bottom { self.scroll_bottom } else { self.rows - 1 };
        self.cursor.row = (self.cursor.row + n).min(bottom);
        self.wrap_next = false;
    }

    pub fn move_left(&mut self, n: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(n);
        self.wrap_next = false;
    }

    pub fn move_right(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.cols - 1);
        self.wrap_next = false;
    }

    pub fn linefeed(&mut self) {
        if self.cursor.row == self.scroll_bottom {
            self.scroll_up(1);
        } else if self.cursor.row + 1 < self.rows {
            self.cursor.row += 1;
        }
    }

    pub fn next_line(&mut self, n: usize) {
        for _ in 0..n.max(1) {
            self.linefeed();
            self.cursor.col = 0;
        }
    }

    pub fn prev_line(&mut self, n: usize) {
        self.move_up(n.max(1));
        self.cursor.col = 0;
    }

    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.wrap_next = false;
    }

    pub fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
        self.wrap_next = false;
    }

    pub fn tab_forward(&mut self, n: usize) {
        for _ in 0..n.max(1) {
            let mut c = self.cursor.col + 1;
            while c < self.cols && !self.tabstops.get(c).copied().unwrap_or(false) {
                c += 1;
            }
            self.cursor.col = c.min(self.cols - 1);
        }
        self.wrap_next = false;
    }

    pub fn tab_backward(&mut self, n: usize) {
        for _ in 0..n.max(1) {
            if self.cursor.col == 0 {
                break;
            }
            let mut c = self.cursor.col - 1;
            while c > 0 && !self.tabstops.get(c).copied().unwrap_or(false) {
                c -= 1;
            }
            self.cursor.col = c;
        }
        self.wrap_next = false;
    }

    pub fn save_cursor(&mut self) {
        self.saved_cursor = (self.cursor.row, self.cursor.col);
    }

    pub fn restore_cursor(&mut self) {
        let (row, col) = self.saved_cursor;
        self.goto(row, col);
    }

    // ----------------------------------------------------------------- stampa

    pub fn print(&mut self, c: char) {
        let width = UnicodeWidthChar::width(c).unwrap_or(1);
        if width == 0 {
            // Segno diacritico combinante: si attacca alla cella precedente.
            let col = self.cursor.col.saturating_sub(1);
            let row = self.cursor.row;
            let mut cell = self.cell(row, col);
            if cell.ch == ' ' {
                cell.ch = c;
                self.pen.flags &= !flags::WIDE_SPACER;
                self.put(row, col, Cell { flags: self.pen.flags, ..cell });
            }
            return;
        }

        if self.wrap_next && self.modes.wrap {
            let row = self.cursor.row;
            let last = self.cols - 1;
            let mut cell = self.cell(row, last);
            cell.flags |= flags::WRAPPED;
            self.put(row, last, cell);
            self.carriage_return();
            self.linefeed();
        }
        self.wrap_next = false;

        if width == 2 && self.cursor.col + 1 >= self.cols {
            if self.modes.wrap {
                let row = self.cursor.row;
                let last = self.cols - 1;
                let mut cell = self.cell(row, last);
                cell.flags |= flags::WRAPPED;
                self.put(row, last, cell);
                self.carriage_return();
                self.linefeed();
            } else {
                return;
            }
        }

        let ch = if self.charset_g[self.charset_active] {
            dec_special(c).unwrap_or(c)
        } else {
            c
        };

        if self.modes.insert {
            let row = self.cursor.row;
            let col = self.cursor.col;
            let shift = if width == 2 { 2 } else { 1 };
            let end = self.cols - shift;
            self.lines[row].copy_within(col..end, col + shift);
            self.dirty.mark(row);
        }

        let pen = self.pen;
        let cell = Cell { ch, flags: pen.flags & !flags::WIDE_SPACER, ..pen };
        let (row, col) = (self.cursor.row, self.cursor.col);
        self.put(row, col, cell);
        if width == 2 && col + 1 < self.cols {
            self.put(row, col + 1, Cell { ch: ' ', flags: pen.flags | flags::WIDE_SPACER, ..pen });
        }
        self.last_print = ch;

        if col + width >= self.cols {
            self.cursor.col = self.cols - 1;
            self.wrap_next = self.modes.wrap;
        } else {
            self.cursor.col = col + width;
        }
        self.epoch += 1;
    }

    pub fn repeat_last(&mut self, n: usize) {
        let c = self.last_print;
        for _ in 0..n {
            self.print(c);
        }
    }

    // ------------------------------------------------------------ cancellazioni

    pub fn erase_line(&mut self, mode: u16) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let blank = self.pen_bg_cell();
        match mode {
            0 => {
                for c in col..self.cols {
                    self.lines[row][c] = blank;
                }
            }
            1 => {
                for c in 0..=col {
                    self.lines[row][c] = blank;
                }
            }
            _ => self.clear_row(row),
        }
        self.dirty.mark(row);
        self.epoch += 1;
    }

    pub fn erase_display(&mut self, mode: u16) {
        let row = self.cursor.row;
        match mode {
            0 => {
                self.erase_line(0);
                for r in row + 1..self.rows {
                    self.clear_row(r);
                }
            }
            1 => {
                self.erase_line(1);
                for r in 0..row {
                    self.clear_row(r);
                }
            }
            _ => {
                for r in 0..self.rows {
                    self.clear_row(r);
                }
                if mode == 3 {
                    self.scrollback.clear();
                    self.display_offset = 0;
                }
            }
        }
        self.dirty.mark_all();
        self.epoch += 1;
    }

    pub fn erase_chars(&mut self, n: usize) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let blank = self.pen_bg_cell();
        for c in col..(col + n).min(self.cols) {
            self.lines[row][c] = blank;
        }
        self.dirty.mark(row);
        self.epoch += 1;
    }

    pub fn insert_chars(&mut self, n: usize) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let n = n.max(1).min(self.cols - col);
        let end = self.cols - n;
        self.lines[row].copy_within(col..end, col + n);
        let blank = self.pen_bg_cell();
        for c in col..col + n {
            self.lines[row][c] = blank;
        }
        self.dirty.mark(row);
        self.epoch += 1;
    }

    pub fn delete_chars(&mut self, n: usize) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let n = n.max(1).min(self.cols - col);
        let blank = self.pen_bg_cell();
        self.lines[row].copy_within(col + n..self.cols, col);
        for c in self.cols - n..self.cols {
            self.lines[row][c] = blank;
        }
        self.dirty.mark(row);
        self.epoch += 1;
    }

    pub fn insert_lines(&mut self, n: usize) {
        if self.cursor.row < self.scroll_top || self.cursor.row > self.scroll_bottom {
            return;
        }
        let blank = self.pen_bg_cell();
        let n = n.max(1).min(self.scroll_bottom - self.cursor.row + 1);
        for _ in 0..n {
            self.lines.remove(self.scroll_bottom);
            self.lines.insert(self.cursor.row, blank_row(self.cols, blank));
        }
        self.dirty.mark_all();
        self.epoch += 1;
    }

    pub fn delete_lines(&mut self, n: usize) {
        if self.cursor.row < self.scroll_top || self.cursor.row > self.scroll_bottom {
            return;
        }
        let blank = self.pen_bg_cell();
        let n = n.max(1).min(self.scroll_bottom - self.cursor.row + 1);
        for _ in 0..n {
            self.lines.remove(self.cursor.row);
            self.lines.insert(self.scroll_bottom, blank_row(self.cols, blank));
        }
        self.dirty.mark_all();
        self.epoch += 1;
    }

    // -------------------------------------------------------- schermo alternato

    pub fn enter_alt(&mut self, save_cursor: bool) {
        if self.saved_lines.is_some() {
            return;
        }
        if save_cursor {
            self.save_cursor();
        }
        let blank = self.blank_cell();
        self.saved_lines = Some(std::mem::replace(
            &mut self.lines,
            (0..self.rows).map(|_| vec![blank; self.cols]).collect(),
        ));
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.dirty.mark_all();
        self.epoch += 1;
    }

    pub fn leave_alt(&mut self) {
        if let Some(mut lines) = self.saved_lines.take() {
            lines.resize_with(self.rows, || vec![self.blank_cell(); self.cols]);
            let blank = self.blank_cell();
            for line in &mut lines {
                line.resize(self.cols, blank);
                line.truncate(self.cols);
            }
            self.lines = lines;
            self.scroll_top = 0;
            self.scroll_bottom = self.rows - 1;
            self.restore_cursor();
            self.dirty.mark_all();
            self.epoch += 1;
        }
    }

    /// Ricolora una griglia già scritta quando cambia il tema: i colori delle celle
    /// sono RGB risolti, quindi vanno rimappati (altrimenti resta il fondo vecchio).
    pub fn retint(
        &mut self,
        old_fg: Rgb,
        old_bg: Rgb,
        old_palette: &[Rgb; 256],
        new_fg: Rgb,
        new_bg: Rgb,
        new_palette: &[Rgb; 256],
    ) {
        let mut map: std::collections::HashMap<Rgb, Rgb> = std::collections::HashMap::with_capacity(300);
        map.insert(old_bg, new_bg);
        map.insert(old_fg, new_fg);
        for i in 0..256 {
            map.entry(old_palette[i]).or_insert(new_palette[i]);
        }
        let conv = |c: Rgb| *map.get(&c).unwrap_or(&c);
        for line in self.lines.iter_mut().chain(self.scrollback.iter_mut()) {
            for cell in line.iter_mut() {
                cell.fg = conv(cell.fg);
                cell.bg = conv(cell.bg);
            }
        }
        if let Some(saved) = self.saved_lines.as_mut() {
            for line in saved.iter_mut() {
                for cell in line.iter_mut() {
                    cell.fg = conv(cell.fg);
                    cell.bg = conv(cell.bg);
                }
            }
        }
        self.default_fg = new_fg;
        self.default_bg = new_bg;
        self.palette = *new_palette;
        self.pen.fg = new_fg;
        self.pen.bg = new_bg;
        self.dirty.mark_all();
        self.epoch += 1;
    }

    // ---------------------------------------------------------------- colore

    pub fn pen_for(&self, fg: Rgb, bg: Rgb, fl: u16) -> Cell {
        Cell { ch: ' ', fg, bg, flags: fl }
    }

    pub fn reset_pen(&mut self) {
        self.pen = Cell::blank(self.default_fg, self.default_bg);
    }

    pub fn full_reset(&mut self) {
        self.modes = Modes::default();
        self.reset_pen();
        self.scroll_top = 0;
        self.scroll_bottom = self.rows - 1;
        self.cursor = Cursor { row: 0, col: 0, style: CursorStyle::Block, visible: true, blink: false };
        self.charset_g = [false, false];
        self.charset_active = 0;
        self.wrap_next = false;
        self.reset_grid();
    }

    // ------------------------------------------------------------ testo/selezione

    /// Numero totale di righe consultabili (scrollback + schermo).
    pub fn total_lines(&self) -> usize {
        self.scrollback.len() + self.rows
    }

    /// Prima riga visibile in coordinate assolute.
    pub fn first_visible_line(&self) -> i64 {
        (self.scrollback.len() as i64) - (self.display_offset as i64)
    }

    pub fn line_cells(&self, line: i64) -> Option<&[Cell]> {
        if line < 0 {
            return None;
        }
        let line = line as usize;
        let sb = self.scrollback.len();
        if line < sb {
            self.scrollback.get(line).map(|v| v.as_slice())
        } else {
            self.lines.get(line - sb).map(|v| v.as_slice())
        }
    }

    /// Testo di una riga, senza spazi in coda.
    pub fn line_text(&self, line: i64) -> String {
        let Some(cells) = self.line_cells(line) else {
            return String::new();
        };
        let mut s = String::with_capacity(cells.len());
        for c in cells {
            if c.is_spacer() {
                continue;
            }
            s.push(c.ch);
        }
        let trimmed = s.trim_end();
        trimmed.to_string()
    }

    /// Testo della selezione (righe intere se `line_mode`).
    pub fn selection_text(&self, from: Point, to: Point) -> String {
        let (a, b) = from.order(to);
        let mut out = String::new();
        let mut line = a.line;
        while line <= b.line {
            let Some(cells) = self.line_cells(line) else {
                line += 1;
                continue;
            };
            let start = if line == a.line { a.col } else { 0 };
            let end = if line == b.line { (b.col + 1).min(cells.len()) } else { cells.len() };
            let wrapped = cells.last().map(|c| c.flags & flags::WRAPPED != 0).unwrap_or(false);
            if start < end {
                for c in &cells[start..end] {
                    if c.is_spacer() {
                        continue;
                    }
                    out.push(c.ch);
                }
            }
            while out.ends_with(' ') && !wrapped {
                out.pop();
            }
            if !wrapped && line < b.line {
                out.push('\n');
            }
            line += 1;
        }
        out
    }

    pub fn select_all_points(&self) -> (Point, Point) {
        let first = 0i64;
        let last = self.total_lines() as i64 - 1;
        (Point::new(first, 0), Point::new(last, self.cols - 1))
    }

    /// Parola (o percorso) sotto il cursore: doppio clic.
    pub fn word_at(&self, line: i64, col: usize) -> (Point, Point) {
        let Some(cells) = self.line_cells(line) else {
            return (Point::new(line, col), Point::new(line, col));
        };
        let is_word = |c: char| {
            c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '~' | '@' | '=' | '?' | '&' | '%' | '+')
        };
        let last = cells.len().saturating_sub(1);
        let mut a = col.min(last);
        while a > 0 && is_word(cells[a - 1].ch) {
            a -= 1;
        }
        let mut b = col.min(last);
        while b + 1 <= last && is_word(cells[b + 1].ch) {
            b += 1;
        }
        (Point::new(line, a), Point::new(line, b))
    }

    /// Indirizzo web presente nella riga, nella colonna indicata (Ctrl+clic).
    pub fn url_at(&self, line: i64, col: usize) -> Option<String> {
        let cells = self.line_cells(line)?;
        let chars: Vec<char> = cells.iter().map(|c| c.ch).collect();
        let text: String = chars.iter().collect();
        let mut i = 0;
        while i < chars.len() {
            let starts = chars[i..].starts_with(&['h', 't', 't', 'p'])
                || chars[i..].starts_with(&['w', 'w', 'w', '.']);
            if !starts {
                i += 1;
                continue;
            }
            let mut end = i;
            while end < chars.len() {
                let c = chars[end];
                if c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '}' | '`') {
                    break;
                }
                end += 1;
            }
            if col >= i && col < end {
                let mut url: String = chars[i..end].iter().collect();
                while url.ends_with('.') || url.ends_with(',') || url.ends_with(';') || url.ends_with(':') {
                    url.pop();
                }
                if url.starts_with("www.") {
                    url = format!("https://{url}");
                }
                if url.starts_with("http://") || url.starts_with("https://") {
                    return Some(url);
                }
                return None;
            }
            i = end.max(i + 1);
        }
        let _ = text;
        None
    }

    /// Etichetta per la scheda: titolo OSC se c'è, altrimenti "Terminale".
    pub fn label(&self) -> String {
        if self.title.is_empty() {
            "Terminale".to_string()
        } else {
            self.title.clone()
        }
    }
}

fn blank_row(cols: usize, blank: Cell) -> Vec<Cell> {
    vec![blank; cols]
}

/// Insieme di caratteri DEC Special Graphics (linee dei riquadri, usatissimo da ncurses).
fn dec_special(c: char) -> Option<char> {
    let r = match c {
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'q' => '─',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'a' => '▒',
        '`' => '◆',
        'f' => '°',
        'g' => '±',
        'o' => '⎺',
        'p' => '⎻',
        'r' => '⎼',
        's' => '⎽',
        '~' => '·',
        '_' => ' ',
        '0' => '█',
        _ => return None,
    };
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn term() -> Terminal {
        Terminal::new(200, 50, 10_000, palette_256([Rgb::new(0, 0, 0); 16]), Rgb::new(220, 220, 220), Rgb::new(20, 20, 25))
    }

    #[test]
    fn stampa_e_a_capo() {
        let mut t = term();
        let mut p = vte::Parser::new();
        p.advance(&mut t, b"ciao\r\nmondo");
        assert_eq!(t.cell(0, 0).ch, 'c');
        assert_eq!(t.cell(1, 0).ch, 'm');
        assert_eq!(t.cursor.row, 1);
    }

    #[test]
    fn a_capo_automatico_e_scrollback() {
        let mut t = term();
        let mut p = vte::Parser::new();
        let line = "x".repeat(250);
        p.advance(&mut t, line.as_bytes());
        // 250 caratteri su 200 colonne: seconda riga, e la prima è marcata "continua"
        assert_eq!(t.cell(0, 199).ch, 'x');
        assert_eq!(t.cursor.row, 1);
        for _ in 0..80 {
            p.advance(&mut t, b"\r\n");
        }
        assert!(!t.scrollback.is_empty());
        assert!(t.scrollback.len() <= 10_000);
    }

    #[test]
    fn colori_e_attributi() {
        let mut t = term();
        let mut p = vte::Parser::new();
        p.advance(&mut t, b"\x1b[1;31mrosso\x1b[0m normale");
        let c = t.cell(0, 0);
        assert_eq!(c.ch, 'r');
        assert!(c.flags & flags::BOLD != 0);
        assert_eq!(c.fg, t.palette[1]);
        assert_eq!(t.cell(0, 5).ch, ' ');
        assert_eq!(t.cell(0, 6).ch, 'n');
        assert_eq!(t.cell(0, 6).flags & flags::BOLD, 0);
    }

    #[test]
    fn vero_colore() {
        let mut t = term();
        let mut p = vte::Parser::new();
        p.advance(&mut t, b"\x1b[38;2;10;20;30mA\x1b[48;5;200mB");
        assert_eq!(t.cell(0, 0).fg, Rgb::new(10, 20, 30));
        assert_eq!(t.cell(0, 1).bg, t.palette[200]);
    }

    #[test]
    fn selezione_unisce_le_righe_continuate() {
        let mut t = term();
        let mut p = vte::Parser::new();
        p.advance(&mut t, format!("{}\r\nseconda", "a".repeat(200)).as_bytes());
        let text = t.selection_text(Point::new(0, 0), Point::new(1, 6));
        // La prima riga era continuata: niente a capo in mezzo alla parola.
        assert_eq!(text.lines().count(), 2, "righe: {text:?}");
        assert!(text.ends_with("seconda"));
    }

    #[test]
    fn schermo_alternato_torna_com_era() {
        let mut t = term();
        let mut p = vte::Parser::new();
        p.advance(&mut t, b"prima\x1b[?1049h poi\x1b[?1049l");
        assert_eq!(t.cell(0, 0).ch, 'p');
        assert!(t.scrollback.is_empty());
    }

    /// Velocità di scarico: si misura con `cargo test --release velocita -- --nocapture`.
    #[test]
    fn velocita_di_parsing() {
        let mut payload = Vec::with_capacity(8 << 20);
        while payload.len() < (8 << 20) {
            payload.extend_from_slice(b"\x1b[1;32mok\x1b[0m riga di prova con un po' di testo e \x1b[38;5;204mcolori\x1b[0m\r\n");
        }
        let mut t = term();
        let mut p = vte::Parser::new();
        let t0 = Instant::now();
        p.advance(&mut t, &payload);
        let dt = t0.elapsed();
        let mb = payload.len() as f64 / (1024.0 * 1024.0);
        println!("scarico: {:.1} MB in {:?} = {:.0} MB/s", mb, dt, mb / dt.as_secs_f64());
        assert!(mb / dt.as_secs_f64() > 20.0, "troppo lento");
    }
}

/// Tavolozza a 256 colori (16 di base xterm + cubo 6×6×6 + grigi).
pub fn palette_256(base: [Rgb; 16]) -> [Rgb; 256] {
    let mut p = [Rgb(0, 0, 0); 256];
    p[..16].copy_from_slice(&base);
    let steps = [0u8, 95, 135, 175, 215, 255];
    let mut i = 16;
    for r in steps {
        for g in steps {
            for b in steps {
                p[i] = Rgb(r, g, b);
                i += 1;
            }
        }
    }
    for k in 0..24u8 {
        let v = 8 + k * 10;
        p[232 + k as usize] = Rgb(v, v, v);
    }
    p
}
