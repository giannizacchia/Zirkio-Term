//! Interpretazione delle sequenze ANSI/ECMA-48 (crate `vte`) applicate alla griglia.

use vte::{Params, Perform};

use super::{CursorStyle, MouseMode, Rgb, Terminal, flags};

/// Parametro grezzo (0 se assente).
fn p(params: &Params, i: usize) -> u16 {
    params.iter().nth(i).and_then(|g| g.first().copied()).unwrap_or(0)
}

/// Parametro con valore minimo (le misure `0` e assenti valgono `def`).
fn pd(params: &Params, i: usize, def: u16) -> usize {
    let v = p(params, i);
    if v == 0 { def as usize } else { v as usize }
}

impl Terminal {
    fn respond(&mut self, bytes: &[u8]) {
        self.events.write.extend_from_slice(bytes);
    }

    fn set_private_mode(&mut self, mode: u16, on: bool) {
        match mode {
            1 => self.modes.app_cursor = on,
            6 => self.modes.origin = on,
            7 => self.modes.wrap = on,
            12 => self.cursor.blink = on,
            25 => self.cursor.visible = on,
            47 | 1047 => {
                if on {
                    self.enter_alt(false);
                } else {
                    self.leave_alt();
                }
            }
            1048 => {
                if on {
                    self.save_cursor();
                } else {
                    self.restore_cursor();
                }
            }
            1049 => {
                if on {
                    self.enter_alt(true);
                    self.erase_display(2);
                } else {
                    self.leave_alt();
                }
            }
            1000 => self.modes.mouse = if on { MouseMode::Click } else { MouseMode::Off },
            1002 => self.modes.mouse = if on { MouseMode::Drag } else { MouseMode::Off },
            1003 => self.modes.mouse = if on { MouseMode::Motion } else { MouseMode::Off },
            1005 => self.modes.utf8_mouse = on,
            1006 => self.modes.sgr_mouse = on,
            1004 => self.modes.focus_events = on,
            1015 => self.modes.utf8_mouse = on,
            2004 => self.modes.bracketed_paste = on,
            1007 => self.modes.alt_scroll = on,
            1034 | 1039 | 1043 | 1009 => {} // mouse "esteso", urxvt: accettati e ignorati
            _ => {}
        }
    }

    fn set_ansi_mode(&mut self, mode: u16, on: bool) {
        if mode == 4 {
            self.modes.insert = on;
        }
    }

    /// Colore da SGR 38/48/58, sia nella forma `;` sia in quella `:`.
    fn sgr_color(&self, group: &[u16], groups: &[&[u16]], i: &mut usize) -> Option<Rgb> {
        if group.len() >= 2 {
            match group[1] {
                5 => group.get(2).map(|n| self.palette[(*n & 0xff) as usize]),
                2 => {
                    let n = group.len();
                    if n >= 5 {
                        Some(Rgb(group[n - 3] as u8, group[n - 2] as u8, group[n - 1] as u8))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            let mode = groups.get(*i + 1).and_then(|g| g.first().copied());
            let at = |k: usize| groups.get(k).and_then(|g| g.first().copied()).unwrap_or(0) as u8;
            match mode {
                Some(5) => {
                    let v = at(*i + 2);
                    *i += 2;
                    Some(self.palette[v as usize])
                }
                Some(2) => {
                    let c = Rgb(at(*i + 2), at(*i + 3), at(*i + 4));
                    *i += 4;
                    Some(c)
                }
                _ => None,
            }
        }
    }

    fn sgr(&mut self, params: &Params) {
        let groups: Vec<&[u16]> = params.iter().collect();
        if groups.is_empty() {
            self.reset_pen();
            return;
        }
        let mut i = 0;
        while i < groups.len() {
            let g = groups[i];
            let code = g[0];
            match code {
                0 => self.reset_pen(),
                1 => self.pen.flags |= flags::BOLD,
                2 => self.pen.flags |= flags::DIM,
                3 => self.pen.flags |= flags::ITALIC,
                4 => self.pen.flags |= flags::UNDERLINE,
                5 | 6 => {}
                7 => self.pen.flags |= flags::INVERSE,
                8 => self.pen.flags |= flags::HIDDEN,
                9 => self.pen.flags |= flags::STRIKE,
                21 | 22 => self.pen.flags &= !(flags::BOLD | flags::DIM),
                23 => self.pen.flags &= !flags::ITALIC,
                24 => self.pen.flags &= !flags::UNDERLINE,
                25 => {}
                27 => self.pen.flags &= !flags::INVERSE,
                28 => self.pen.flags &= !flags::HIDDEN,
                29 => self.pen.flags &= !flags::STRIKE,
                30..=37 => self.pen.fg = self.palette[(code - 30) as usize],
                38 => {
                    if let Some(c) = self.sgr_color(g, &groups, &mut i) {
                        self.pen.fg = c;
                    }
                }
                39 => self.pen.fg = self.default_fg,
                40..=47 => self.pen.bg = self.palette[(code - 40) as usize],
                48 => {
                    if let Some(c) = self.sgr_color(g, &groups, &mut i) {
                        self.pen.bg = c;
                    }
                }
                49 => self.pen.bg = self.default_bg,
                58 => {
                    let _ = self.sgr_color(g, &groups, &mut i);
                }
                59 => {}
                90..=97 => self.pen.fg = self.palette[(code - 90 + 8) as usize],
                100..=107 => self.pen.bg = self.palette[(code - 100 + 8) as usize],
                _ => {}
            }
            i += 1;
        }
    }

    fn device_status(&mut self, what: u16) {
        match what {
            5 => self.respond(b"\x1b[0n"),
            6 => {
                let (r, c) = (self.cursor.row + 1, self.cursor.col + 1);
                let s = format!("\x1b[{r};{c}R");
                self.respond(s.as_bytes());
            }
            _ => {}
        }
    }

    fn csi(&mut self, params: &Params, intermediates: &[u8], action: char) {
        match action {
            '@' => self.insert_chars(pd(params, 0, 1)),
            'A' => self.move_up(pd(params, 0, 1)),
            'B' => self.move_down(pd(params, 0, 1)),
            'C' => self.move_right(pd(params, 0, 1)),
            'D' => self.move_left(pd(params, 0, 1)),
            'E' => self.next_line(pd(params, 0, 1)),
            'F' => self.prev_line(pd(params, 0, 1)),
            'G' | '`' => {
                let col = pd(params, 0, 1) - 1;
                let row = self.cursor.row;
                self.goto(row, col);
            }
            'H' | 'f' => {
                let row = pd(params, 0, 1) - 1;
                let col = pd(params, 1, 1) - 1;
                self.goto(row, col);
            }
            'I' => self.tab_forward(pd(params, 0, 1)),
            'J' => self.erase_display(p(params, 0)),
            'K' => self.erase_line(p(params, 0)),
            'L' => self.insert_lines(pd(params, 0, 1)),
            'M' => self.delete_lines(pd(params, 0, 1)),
            'P' => self.delete_chars(pd(params, 0, 1)),
            'S' => self.scroll_up(pd(params, 0, 1)),
            'T' => self.scroll_down(pd(params, 0, 1)),
            'X' => self.erase_chars(pd(params, 0, 1)),
            'Z' => self.tab_backward(pd(params, 0, 1)),
            'b' => self.repeat_last(pd(params, 0, 1)),
            'd' => {
                let row = pd(params, 0, 1) - 1;
                let col = self.cursor.col;
                self.goto(row, col);
            }
            'g' => match p(params, 0) {
                0 => {
                    let col = self.cursor.col;
                    if let Some(t) = self.tabstops.get_mut(col) {
                        *t = false;
                    }
                }
                3 => self.tabstops.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            'h' | 'l' => {
                let on = action == 'h';
                if intermediates.first() == Some(&b'?') {
                    let modes: Vec<u16> = params.iter().filter_map(|g| g.first().copied()).collect();
                    for m in modes {
                        self.set_private_mode(m, on);
                    }
                } else {
                    let modes: Vec<u16> = params.iter().filter_map(|g| g.first().copied()).collect();
                    for m in modes {
                        self.set_ansi_mode(m, on);
                    }
                }
            }
            'm' => self.sgr(params),
            'n' => self.device_status(p(params, 0)),
            'r' => {
                let top = pd(params, 0, 1) - 1;
                let bottom = pd(params, 1, self.rows as u16) - 1;
                self.set_scroll_region(top, bottom);
            }
            's' => self.save_cursor(),
            'u' => self.restore_cursor(),
            'c' => {
                if p(params, 0) == 0 {
                    self.respond(b"\x1b[?62;22c");
                }
            }
            'q' => {
                if intermediates.first() == Some(&b' ') {
                    self.cursor.style = match p(params, 0) {
                        3 | 4 => CursorStyle::Underline,
                        5 | 6 => CursorStyle::Beam,
                        _ => CursorStyle::Block,
                    };
                }
            }
            't' => match p(params, 0) {
                18 => {
                    let s = format!("\x1b[8;{};{}t", self.rows, self.cols);
                    self.respond(s.as_bytes());
                }
                14 => {
                    let s = format!("\x1b[4;{};{}t", self.rows, self.cols);
                    self.respond(s.as_bytes());
                }
                _ => {}
            },
            'p' => {
                if intermediates.first() == Some(&b'!') && p(params, 0) == 0 {
                    // DECSTR: ripristino "morbido"
                    self.modes.origin = false;
                    self.modes.insert = false;
                    self.modes.app_cursor = false;
                    self.modes.app_keypad = false;
                    self.wrap_next = false;
                    self.cursor.visible = true;
                    self.cursor.style = CursorStyle::Block;
                    self.scroll_top = 0;
                    self.scroll_bottom = self.rows - 1;
                    self.reset_pen();
                }
            }
            _ => {}
        }
    }
}

impl Perform for Terminal {
    fn print(&mut self, c: char) {
        Terminal::print(self, c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x07 => self.events.bell = true,
            0x08 => self.backspace(),
            0x09 => self.tab_forward(1),
            0x0a | 0x0b | 0x0c => self.linefeed(),
            0x0d => self.carriage_return(),
            0x0e => self.charset_active = 1,
            0x0f => self.charset_active = 0,
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        self.csi(params, intermediates, action);
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates.first(), byte) {
            (None, b'7') => self.save_cursor(),
            (None, b'8') => self.restore_cursor(),
            (None, b'D') => self.linefeed(),
            (None, b'E') => {
                self.carriage_return();
                self.linefeed();
            }
            (None, b'H') => {
                let col = self.cursor.col;
                if let Some(t) = self.tabstops.get_mut(col) {
                    *t = true;
                }
            }
            (None, b'M') => {
                if self.cursor.row == self.scroll_top {
                    self.scroll_down(1);
                } else if self.cursor.row > 0 {
                    self.cursor.row -= 1;
                }
            }
            (None, b'c') => self.full_reset(),
            (None, b'=') => self.modes.app_keypad = true,
            (None, b'>') => self.modes.app_keypad = false,
            (Some(b'#'), b'8') => {
                // DECALN: schermo pieno di 'E' (prova dell'allineamento)
                let cell = self.pen;
                for row in 0..self.rows {
                    for col in 0..self.cols {
                        let c = super::Cell { ch: 'E', ..cell };
                        self.lines[row][col] = c;
                    }
                }
                self.dirty.mark_all();
            }
            (Some(b'('), _) => self.charset_g[0] = byte == b'0',
            (Some(b')'), _) => self.charset_g[1] = byte == b'0',
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(kind) = params.first() else { return };
        match *kind {
            b"0" | b"2" => {
                if let Some(title) = params.get(1) {
                    let t = String::from_utf8_lossy(title).to_string();
                    self.title = t.clone();
                    self.events.title = Some(t);
                }
            }
            b"52" => {
                if let (Some(_sel), Some(data)) = (params.get(1), params.get(2)) {
                    if let Some(text) = base64_decode(data) {
                        self.events.clipboard = Some(text);
                    }
                } else if params.len() >= 2 {
                    self.events.clipboard = Some(String::new());
                }
            }
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}
}

fn base64_decode(data: &[u8]) -> Option<String> {
    fn val(b: u8) -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &b in data {
        if b == b'=' {
            break;
        }
        let Some(v) = val(b) else { continue };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    String::from_utf8(out).ok()
}
