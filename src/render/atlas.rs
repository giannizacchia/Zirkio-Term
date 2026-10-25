//! Atlante dei glifi: una sola texture `R8` con tutti i caratteri già rasterizzati.
//!
//! Ogni glifo si rasterizza **una volta sola** e resta in cache come coordinate UV;
//! disegnare una schermata significa solo scrivere dei quad.

use std::collections::HashMap;

use glow::HasContext;
use swash::FontRef;
use swash::scale::{Render as GlyphRender, ScaleContext, Source};
use swash::zeno::Format;

use super::font::FontSet;

/// Un glifo pronto per il disegno.
#[derive(Clone, Copy, Default)]
pub struct Glyph {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    /// Larghezza/altezza dell'inchiostro in pixel.
    pub w: f32,
    pub h: f32,
    /// Scostamento dall'angolo in alto a sinistra della cella.
    pub left: f32,
    pub top: f32,
}

pub struct Atlas {
    tex: glow::Texture,
    size: i32,
    x: i32,
    y: i32,
    row_h: i32,
    map: HashMap<(u32, u8), Option<Glyph>>,
    ctx: ScaleContext,
    /// Il pixel bianco pieno (usato dai rettangoli pieni).
    pub white: (f32, f32),
    /// L'atlante è stato azzerato: serve un ridisegno completo.
    pub reset: bool,
}

const ATLAS_SIZE: i32 = 1024;

impl Atlas {
    pub fn new(gl: &glow::Context) -> Result<Atlas, String> {
        unsafe {
            let tex = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                ATLAS_SIZE,
                ATLAS_SIZE,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            // In alto a sinistra un pixel bianco pieno: i rettangoli pieni lo campionano.
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                1,
                1,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&[255u8])),
            );
            Ok(Atlas {
                tex,
                size: ATLAS_SIZE,
                x: 1,
                y: 0,
                row_h: 1,
                map: HashMap::with_capacity(512),
                ctx: ScaleContext::new(),
                white: (0.5 / ATLAS_SIZE as f32, 0.5 / ATLAS_SIZE as f32),
                reset: false,
            })
        }
    }

    pub fn texture(&self) -> glow::Texture {
        self.tex
    }

    /// Glifo già pronto, oppure rasterizzato adesso.
    pub fn glyph(&mut self, gl: &glow::Context, fonts: &FontSet, ch: char, style: u8) -> Option<Glyph> {
        let key = (ch as u32, style);
        if let Some(hit) = self.map.get(&key) {
            return *hit;
        }
        let g = self.rasterize(gl, fonts, ch, style);
        self.map.insert(key, g);
        g
    }

    fn rasterize(&mut self, gl: &glow::Context, fonts: &FontSet, ch: char, style: u8) -> Option<Glyph> {
        if ch == ' ' || ch == '\u{00a0}' {
            return None;
        }
        let (bytes, faux_bold, _faux_italic) = fonts.data_for(style);
        let mut font = FontRef::from_index(bytes, 0)?;
        let mut glyph_id = font.charmap().map(ch);
        if glyph_id == 0 {
            // Carattere assente: si prova il font di riserva (riquadri, frecce…).
            if let Some(fb) = fonts.fallback_data() {
                if let Some(f2) = FontRef::from_index(fb, 0) {
                    let id = f2.charmap().map(ch);
                    if id != 0 {
                        font = f2;
                        glyph_id = id;
                    }
                }
            }
        }
        if glyph_id == 0 {
            glyph_id = font.charmap().map('\u{fffd}');
            if glyph_id == 0 {
                glyph_id = font.charmap().map('?');
            }
            if glyph_id == 0 {
                return None;
            }
        }
        let mut scaler = self.ctx.builder(font).size(fonts.size).hint(true).build();
        let mut render = GlyphRender::new(&[Source::Outline]);
        render.format(Format::Alpha);
        if faux_bold {
            render.embolden(fonts.bold_strength);
        }
        let image = render.render(&mut scaler, glyph_id)?;
        let w = image.placement.width as i32;
        let h = image.placement.height as i32;
        if w == 0 || h == 0 {
            return None;
        }
        let (ax, ay) = self.alloc(w, h)?;
        // `Content::Mask` = un byte per pixel; i colori (emoji) sarebbero 4.
        if image.data.len() == (w * h) as usize {
            unsafe {
                gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
                gl.tex_sub_image_2d(
                    glow::TEXTURE_2D,
                    0,
                    ax,
                    ay,
                    w,
                    h,
                    glow::RED,
                    glow::UNSIGNED_BYTE,
                    glow::PixelUnpackData::Slice(Some(&image.data)),
                );
            }
        }
        let s = self.size as f32;
        Some(Glyph {
            u0: ax as f32 / s,
            v0: ay as f32 / s,
            u1: (ax + w) as f32 / s,
            v1: (ay + h) as f32 / s,
            w: w as f32,
            h: h as f32,
            left: image.placement.left as f32,
            top: image.placement.top as f32,
        })
    }

    /// Posto per un'immagine `w`×`h` (impilamento a ripiani).
    fn alloc(&mut self, w: i32, h: i32) -> Option<(i32, i32)> {
        if w > self.size {
            return None;
        }
        if self.x + w > self.size {
            self.x = 1;
            self.y += self.row_h.max(1);
            self.row_h = 1;
        }
        if self.y + h > self.size {
            // Pieno: si ricomincia. Chi disegna se ne accorge da `reset`.
            self.map.clear();
            self.x = 1;
            self.y = 0;
            self.row_h = 1;
            self.reset = true;
            return None;
        }
        let (ax, ay) = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        Some((ax, ay))
    }

    /// Scalda la cache con i caratteri che si vedono sempre (avvio più liscio).
    pub fn prewarm(&mut self, gl: &glow::Context, fonts: &FontSet) {
        for c in 0x20u8..0x7f {
            self.glyph(gl, fonts, c as char, 0);
            self.glyph(gl, fonts, c as char, super::font::STYLE_BOLD);
        }
        for c in ['\u{2500}', '\u{2502}', '\u{250c}', '\u{2510}', '\u{2514}', '\u{2518}', '\u{2588}', '\u{2591}', '\u{25b6}', '\u{2026}', '\u{00b7}', '\u{2192}', '\u{2190}'] {
            self.glyph(gl, fonts, c, 0);
        }
    }

    pub fn take_reset(&mut self) -> bool {
        let r = self.reset;
        self.reset = false;
        r
    }

    /// Svuota la texture e la cache: serve quando cambia il font o la dimensione.
    pub fn clear(&mut self, gl: &glow::Context) {
        self.map.clear();
        self.x = 1;
        self.y = 0;
        self.row_h = 1;
        self.reset = false;
        unsafe {
            let zeros = vec![0u8; (self.size * self.size) as usize];
            gl.bind_texture(glow::TEXTURE_2D, Some(self.tex));
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                self.size,
                self.size,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&zeros)),
            );
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                1,
                1,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&[255u8])),
            );
        }
    }
}
