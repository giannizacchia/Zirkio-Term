//! Da tasto premuto (keysym X11/GDK) a byte per il processo: protocollo xterm.

use super::{MouseMode, Modes};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub sup: bool,
}

/// Keysym X11: sono gli stessi valori che GDK mette in `EventKey::keyval()`.
pub mod keys {
    pub const RETURN: u32 = 0xff0d;
    pub const KP_ENTER: u32 = 0xff8d;
    pub const TAB: u32 = 0xff09;
    pub const ISO_LEFT_TAB: u32 = 0xfe20;
    pub const BACKSPACE: u32 = 0xff08;
    pub const ESCAPE: u32 = 0xff1b;
    pub const SPACE: u32 = 0x0020;
    pub const UP: u32 = 0xff52;
    pub const DOWN: u32 = 0xff54;
    pub const LEFT: u32 = 0xff51;
    pub const RIGHT: u32 = 0xff53;
    pub const HOME: u32 = 0xff50;
    pub const END: u32 = 0xff57;
    pub const INSERT: u32 = 0xff63;
    pub const DELETE: u32 = 0xffff;
    pub const PAGE_UP: u32 = 0xff55;
    pub const PAGE_DOWN: u32 = 0xff56;
    pub const F1: u32 = 0xffbe;
    pub const F2: u32 = 0xffbf;
    pub const F3: u32 = 0xffc0;
    pub const F4: u32 = 0xffc1;
    pub const F5: u32 = 0xffc2;
    pub const F6: u32 = 0xffc3;
    pub const F7: u32 = 0xffc4;
    pub const F8: u32 = 0xffc5;
    pub const F9: u32 = 0xffc6;
    pub const F10: u32 = 0xffc7;
    pub const F11: u32 = 0xffc8;
    pub const F12: u32 = 0xffc9;
}

/// Modificatori come li vuole xterm nei CSI (`1;{m}`).
fn mod_num(m: Mods) -> u8 {
    let mut n = 1;
    if m.shift {
        n += 1;
    }
    if m.alt {
        n += 2;
    }
    if m.ctrl {
        n += 4;
    }
    if m.sup {
        n += 8;
    }
    n
}

fn csi_final(buf: &mut Vec<u8>, final_byte: u8, m: Mods) {
    let n = mod_num(m);
    if n > 1 {
        buf.extend_from_slice(format!("\x1b[1;{n}").as_bytes());
        buf.push(final_byte);
    } else {
        buf.extend_from_slice(b"\x1b[");
        buf.push(final_byte);
    }
}

fn csi_tilde(buf: &mut Vec<u8>, num: u8, m: Mods) {
    let n = mod_num(m);
    if n > 1 {
        buf.extend_from_slice(format!("\x1b[{num};{n}~").as_bytes());
    } else {
        buf.extend_from_slice(format!("\x1b[{num}~").as_bytes());
    }
}

fn arrow(buf: &mut Vec<u8>, final_byte: u8, m: Mods, app_cursor: bool) {
    let n = mod_num(m);
    if n > 1 {
        csi_final(buf, final_byte, m);
    } else if app_cursor {
        buf.extend_from_slice(&[0x1b, b'O', final_byte]);
    } else {
        buf.extend_from_slice(&[0x1b, b'[', final_byte]);
    }
}

fn control_char(c: char) -> Option<u8> {
    let c = c.to_ascii_lowercase();
    let v = match c {
        'a'..='z' => c as u8 - b'a' + 1,
        '@' | ' ' | '2' => 0x00,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '8' => 0x7f,
        _ => return None,
    };
    Some(v)
}

/// Il carattere che rappresenta un keysym (latino-1 e Unicode diretto).
pub fn keyval_char(kv: u32) -> Option<char> {
    if (0x20..=0x7e).contains(&kv) || (0xa0..=0xff).contains(&kv) {
        char::from_u32(kv)
    } else if kv >= 0x0100_0000 {
        char::from_u32(kv - 0x0100_0000)
    } else {
        None
    }
}

/// Byte da mandare al processo per un tasto.
pub fn encode_key(keyval: Option<u32>, m: Mods, modes: &Modes) -> Option<Vec<u8>> {
    let kv = keyval?;
    let mut out = Vec::with_capacity(8);

    // Tasti con un nome
    let named = match kv {
        keys::RETURN | keys::KP_ENTER => Some(0u8),
        keys::TAB => Some(1),
        keys::ISO_LEFT_TAB => Some(2),
        keys::BACKSPACE => Some(3),
        keys::ESCAPE => Some(4),
        keys::UP => Some(5),
        keys::DOWN => Some(6),
        keys::RIGHT => Some(7),
        keys::LEFT => Some(8),
        keys::HOME => Some(9),
        keys::END => Some(10),
        keys::INSERT => Some(11),
        keys::DELETE => Some(12),
        keys::PAGE_UP => Some(13),
        keys::PAGE_DOWN => Some(14),
        keys::F1..=keys::F12 => Some(15 + (kv - keys::F1) as u8),
        _ => None,
    };
    if let Some(which) = named {
        match which {
            0 => out.push(b'\r'),
            1 => {
                if m.shift {
                    out.extend_from_slice(b"\x1b[Z");
                } else {
                    out.push(b'\t');
                }
            }
            2 => out.extend_from_slice(b"\x1b[Z"),
            3 => out.push(0x7f),
            4 => out.push(0x1b),
            5 => arrow(&mut out, b'A', m, modes.app_cursor),
            6 => arrow(&mut out, b'B', m, modes.app_cursor),
            7 => arrow(&mut out, b'C', m, modes.app_cursor),
            8 => arrow(&mut out, b'D', m, modes.app_cursor),
            9 => {
                if modes.app_cursor {
                    out.extend_from_slice(b"\x1bOH");
                } else {
                    csi_final(&mut out, b'H', m);
                }
            }
            10 => {
                if modes.app_cursor {
                    out.extend_from_slice(b"\x1bOF");
                } else {
                    csi_final(&mut out, b'F', m);
                }
            }
            11 => csi_tilde(&mut out, 2, m),
            12 => csi_tilde(&mut out, 3, m),
            13 => csi_tilde(&mut out, 5, m),
            14 => csi_tilde(&mut out, 6, m),
            n if n >= 15 => {
                let f = n - 15;
                match f {
                    // SS3 non ha posto per i modificatori: con Shift/Ctrl/Alt xterm
                    // passa a CSI 1;<m>P..S (es. Ctrl-F4 = "\x1b[1;5S").
                    0..=3 if m.shift || m.ctrl || m.alt || m.sup => {
                        csi_final(&mut out, b'P' + f as u8, m);
                    }
                    0 => out.extend_from_slice(b"\x1bOP"),
                    1 => out.extend_from_slice(b"\x1bOQ"),
                    2 => out.extend_from_slice(b"\x1bOR"),
                    3 => out.extend_from_slice(b"\x1bOS"),
                    4 => csi_tilde(&mut out, 15, m),
                    5 => csi_tilde(&mut out, 17, m),
                    6 => csi_tilde(&mut out, 18, m),
                    7 => csi_tilde(&mut out, 19, m),
                    8 => csi_tilde(&mut out, 20, m),
                    9 => csi_tilde(&mut out, 21, m),
                    10 => csi_tilde(&mut out, 23, m),
                    _ => csi_tilde(&mut out, 24, m),
                }
            }
            _ => return None,
        }
        if m.alt && !matches!(which, 4) {
            out.insert(0, 0x1b);
        }
        return Some(out);
    }

    // Tasti con un carattere
    let ch = keyval_char(kv)?;
    if m.ctrl {
        if let Some(b) = control_char(ch) {
            if m.alt {
                out.push(0x1b);
            }
            out.push(b);
            return Some(out);
        }
    }
    if kv == keys::SPACE && !m.ctrl {
        out.extend_from_slice(b" ");
        if m.alt {
            out.insert(0, 0x1b);
        }
        return Some(out);
    }
    let mut bytes = ch.to_string().into_bytes();
    if m.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseKind {
    Press(u8),
    Release(u8),
    Motion(u8),
    WheelUp,
    WheelDown,
}

/// Byte per il processo se l'applicazione ha chiesto il mouse, altrimenti `None`.
pub fn encode_mouse(modes: &Modes, kind: MouseKind, col: usize, row: usize, m: Mods) -> Option<Vec<u8>> {
    if modes.mouse == MouseMode::Off {
        return None;
    }
    let mut cb: u16 = match kind {
        MouseKind::Press(b) => match b {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => return None,
        },
        MouseKind::Release(_) => 3,
        MouseKind::Motion(b) => match b {
            0 => 32,
            1 => 33,
            2 => 34,
            _ => 35,
        },
        MouseKind::WheelUp => 64,
        MouseKind::WheelDown => 65,
    };
    if m.shift {
        cb += 4;
    }
    if m.alt {
        cb += 8;
    }
    if m.ctrl {
        cb += 16;
    }
    let (cx, cy) = (col + 1, row + 1);
    if modes.sgr_mouse {
        let finale = if matches!(kind, MouseKind::Release(_)) { 'm' } else { 'M' };
        Some(format!("\x1b[<{cb};{cx};{cy}{finale}").into_bytes())
    } else {
        if cx > 223 || cy > 223 {
            return None;
        }
        Some(vec![0x1b, b'[', b'M', (32 + cb) as u8, (32 + cx) as u8, (32 + cy) as u8])
    }
}

/// Testo incollato, con la "parentesi" se l'applicazione l'ha chiesta.
pub fn encode_paste(modes: &Modes, text: &str) -> Vec<u8> {
    let clean = text.replace("\r\n", "\r").replace('\n', "\r");
    if modes.bracketed_paste {
        let mut v = Vec::with_capacity(clean.len() + 12);
        v.extend_from_slice(b"\x1b[200~");
        v.extend_from_slice(clean.as_bytes());
        v.extend_from_slice(b"\x1b[201~");
        v
    } else {
        clean.into_bytes()
    }
}

pub fn encode_focus(modes: &Modes, focused: bool) -> Option<Vec<u8>> {
    if !modes.focus_events {
        return None;
    }
    Some(if focused { b"\x1b[I".to_vec() } else { b"\x1b[O".to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes() -> Modes {
        Modes::default()
    }

    #[test]
    fn ctrl_lettera_e_control_code() {
        let bytes = encode_key(Some('a' as u32), Mods { ctrl: true, ..Default::default() }, &modes()).unwrap();
        assert_eq!(bytes, vec![1]);
    }

    #[test]
    fn frecce_csi_e_ss3() {
        let m = Mods::default();
        assert_eq!(encode_key(Some(keys::UP), m, &modes()).unwrap(), b"\x1b[A");
        let mut mo = modes();
        mo.app_cursor = true;
        assert_eq!(encode_key(Some(keys::UP), m, &mo).unwrap(), b"\x1bOA");
    }

    #[test]
    fn tasti_f_con_modificatore() {
        let m = Mods::default();
        assert_eq!(encode_key(Some(keys::F4), m, &modes()).unwrap(), b"\x1bOS");
        let ctrl = Mods { ctrl: true, ..Default::default() };
        // Ctrl-F4: SS3 non porta i modificatori, xterm usa CSI 1;5S
        assert_eq!(encode_key(Some(keys::F4), ctrl, &modes()).unwrap(), b"\x1b[1;5S");
        let shift = Mods { shift: true, ..Default::default() };
        assert_eq!(encode_key(Some(keys::F1), shift, &modes()).unwrap(), b"\x1b[1;2P");
        // F5 e oltre passavano già dalle CSI con il modificatore
        assert_eq!(encode_key(Some(keys::F5), ctrl, &modes()).unwrap(), b"\x1b[15;5~");
    }

    #[test]
    fn alt_prefisso() {
        let m = Mods { alt: true, ..Default::default() };
        assert_eq!(encode_key(Some('b' as u32), m, &modes()).unwrap(), vec![0x1b, b'b']);
    }

    #[test]
    fn carattere_unicode() {
        let m = Mods::default();
        let kv = 0x0100_0000 + 'è' as u32;
        assert_eq!(encode_key(Some(kv), m, &modes()).unwrap(), "è".as_bytes());
    }

    #[test]
    fn canc_e_invio() {
        let m = Mods::default();
        assert_eq!(encode_key(Some(keys::BACKSPACE), m, &modes()).unwrap(), vec![0x7f]);
        assert_eq!(encode_key(Some(keys::RETURN), m, &modes()).unwrap(), vec![b'\r']);
        assert_eq!(encode_key(Some(keys::DELETE), m, &modes()).unwrap(), b"\x1b[3~");
    }
}
