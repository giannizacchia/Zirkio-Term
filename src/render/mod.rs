//! Disegno: un solo programma shader, un buffer di istanze per la griglia e uno per
//! i ritocchi (cursore). Si riscrive **solo la riga cambiata**: il resto del buffer
//! resta valido com'era, quindi una schermata ferma non costa niente.

use std::rc::Rc;

use glow::HasContext;

/// Istanze disegnate per ogni cella: sfondo + 4 barre (linee singole, doppie, incroci).
pub const INST_PER_CELL: usize = 5;

pub mod atlas;
pub mod font;
pub mod loader;

use atlas::{Atlas, Glyph};
use font::{FontSet, STYLE_BOLD, STYLE_ITALIC};

use crate::term::{Cell, Rgb, flags};

/// Carica `libGL` e tiene vivi i puntatori: la libreria non va scaricata.
pub struct GlLoaderOwned {
    pub loader: loader::GlLoader,
}

impl GlLoaderOwned {
    pub fn new() -> Result<GlLoaderOwned, String> {
        Ok(GlLoaderOwned { loader: loader::GlLoader::new()? })
    }
}

const VERTEX: &str = r#"#version 330 core
layout(location = 0) in vec2 a_pos;
layout(location = 1) in vec2 a_uv;
layout(location = 2) in vec2 i_pos;
layout(location = 3) in vec2 i_size;
layout(location = 4) in vec4 i_color;
layout(location = 5) in vec4 i_uv;
uniform vec2 u_res;
out vec2 v_uv;
out vec4 v_color;
void main() {
    vec2 px = i_pos + a_pos * i_size;
    vec2 ndc = vec2(px.x / u_res.x * 2.0 - 1.0, 1.0 - px.y / u_res.y * 2.0);
    gl_Position = vec4(ndc, 0.0, 1.0);
    v_uv = mix(i_uv.xy, i_uv.zw, a_uv);
    v_color = i_color;
}
"#;

const FRAGMENT: &str = r#"#version 330 core
in vec2 v_uv;
in vec4 v_color;
uniform sampler2D u_atlas;
out vec4 o_color;
void main() {
    float a = texture(u_atlas, v_uv).r;
    o_color = vec4(v_color.rgb, v_color.a * a);
}
"#;

/// Un rettangolo: posizione, misura, colore, coordinate nell'atlante.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Instance {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// Linee da riquadro: (sinistra, destra, su, giu', spessore).
/// 0 = niente, 1 = linea singola, 2 = linea doppia. Le linee le disegniamo noi: il
/// glifo del font non copre l'intera cella e le verticali risultano tratteggiate
/// (misurato su mc: 504 px di fila in orizzontale contro 9 in verticale).
fn box_parts(ch: char) -> Option<(u8, u8, u8, u8, f32)> {
    Some(match ch {
        // sottili
        '\u{2500}' => (1, 1, 0, 0, 1.0),
        '\u{2502}' => (0, 0, 1, 1, 1.0),
        '\u{250c}' => (0, 1, 0, 1, 1.0),
        '\u{2510}' => (1, 0, 0, 1, 1.0),
        '\u{2514}' => (0, 1, 1, 0, 1.0),
        '\u{2518}' => (1, 0, 1, 0, 1.0),
        '\u{251c}' => (0, 1, 1, 1, 1.0),
        '\u{2524}' => (1, 0, 1, 1, 1.0),
        '\u{252c}' => (1, 1, 0, 1, 1.0),
        '\u{2534}' => (1, 1, 1, 0, 1.0),
        '\u{253c}' => (1, 1, 1, 1, 1.0),
        // spesse
        '\u{2501}' => (1, 1, 0, 0, 1.8),
        '\u{2503}' => (0, 0, 1, 1, 1.8),
        '\u{250f}' => (0, 1, 0, 1, 1.8),
        '\u{2513}' => (1, 0, 0, 1, 1.8),
        '\u{2517}' => (0, 1, 1, 0, 1.8),
        '\u{251b}' => (1, 0, 1, 0, 1.8),
        '\u{2523}' => (0, 1, 1, 1, 1.8),
        '\u{252b}' => (1, 0, 1, 1, 1.8),
        '\u{2533}' => (1, 1, 0, 1, 1.8),
        '\u{253b}' => (1, 1, 1, 0, 1.8),
        '\u{254b}' => (1, 1, 1, 1, 1.8),
        // doppie (quelle che mc usa quando la lingua e' UTF-8)
        '\u{2550}' => (2, 2, 0, 0, 1.0),
        '\u{2551}' => (0, 0, 2, 2, 1.0),
        '\u{2552}' => (0, 1, 0, 1, 1.0),
        '\u{2553}' => (0, 1, 0, 2, 1.0),
        '\u{2554}' => (0, 2, 0, 2, 1.0),
        '\u{2555}' => (1, 0, 0, 1, 1.0),
        '\u{2556}' => (1, 0, 0, 2, 1.0),
        '\u{2557}' => (2, 0, 0, 2, 1.0),
        '\u{2558}' => (0, 1, 1, 0, 1.0),
        '\u{2559}' => (0, 1, 2, 0, 1.0),
        '\u{255a}' => (0, 2, 2, 0, 1.0),
        '\u{255b}' => (1, 0, 1, 0, 1.0),
        '\u{255c}' => (1, 0, 2, 0, 1.0),
        '\u{255d}' => (2, 0, 2, 0, 1.0),
        '\u{255e}' => (0, 1, 1, 1, 1.0),
        '\u{255f}' => (0, 1, 2, 2, 1.0),
        '\u{2560}' => (0, 2, 2, 2, 1.0),
        '\u{2561}' => (1, 0, 1, 1, 1.0),
        '\u{2562}' => (1, 0, 2, 2, 1.0),
        '\u{2563}' => (2, 0, 2, 2, 1.0),
        '\u{2564}' => (1, 1, 0, 1, 1.0),
        '\u{2565}' => (1, 1, 0, 2, 1.0),
        '\u{2566}' => (2, 2, 0, 2, 1.0),
        '\u{2567}' => (1, 1, 1, 0, 1.0),
        '\u{2568}' => (1, 1, 2, 0, 1.0),
        '\u{2569}' => (2, 2, 2, 0, 1.0),
        '\u{256a}' => (2, 2, 1, 1, 1.0),
        '\u{256b}' => (1, 1, 2, 2, 1.0),
        '\u{256c}' => (2, 2, 2, 2, 1.0),
        _ => return None,
    })
}

/// Blocchi e sfumature: (x0, y0, x1, y1 in frazioni di cella, densita' 0-255).
fn block_rect(ch: char) -> Option<(f32, f32, f32, f32, u8)> {
    Some(match ch {
        '\u{2588}' => (0.0, 0.0, 1.0, 1.0, 255),
        '\u{2580}' => (0.0, 0.0, 1.0, 0.5, 255),
        '\u{2584}' => (0.0, 0.5, 1.0, 1.0, 255),
        '\u{258c}' => (0.0, 0.0, 0.5, 1.0, 255),
        '\u{2590}' => (0.5, 0.0, 1.0, 1.0, 255),
        '\u{2591}' => (0.0, 0.0, 1.0, 1.0, 64),
        '\u{2592}' => (0.0, 0.0, 1.0, 1.0, 128),
        '\u{2593}' => (0.0, 0.0, 1.0, 1.0, 192),
        '\u{2581}' => (0.0, 0.875, 1.0, 1.0, 255),
        '\u{2582}' => (0.0, 0.75, 1.0, 1.0, 255),
        '\u{2583}' => (0.0, 0.625, 1.0, 1.0, 255),
        '\u{2585}' => (0.0, 0.375, 1.0, 1.0, 255),
        '\u{2586}' => (0.0, 0.25, 1.0, 1.0, 255),
        '\u{2587}' => (0.0, 0.125, 1.0, 1.0, 255),
        _ => return None,
    })
}

impl Instance {
    #[inline]
    fn solid(x: f32, y: f32, w: f32, h: f32, color: Rgb, white: (f32, f32)) -> Instance {
        let [r, g, b, a] = color.rgba();
        Instance { x, y, w, h, r, g, b, a, u0: white.0, v0: white.1, u1: white.0, v1: white.1 }
    }
}

pub struct Renderer {
    pub gl: glow::Context,
    program: glow::Program,
    u_res: Option<glow::UniformLocation>,
    u_atlas: Option<glow::UniformLocation>,
    vao_grid: glow::VertexArray,
    vbo_grid: glow::Buffer,
    vao_overlay: glow::VertexArray,
    vbo_overlay: glow::Buffer,
    vbo_quad: glow::Buffer,
    pub atlas: Atlas,
    pub fonts: Rc<FontSet>,
    pub width: i32,
    pub height: i32,
    grid_capacity: usize,
    grid_count: usize,
    /// Quante celle per riga sono state riservate nel buffer della griglia.
    grid_cols: usize,
}

impl Renderer {
    pub fn new(gl: glow::Context, fonts: Rc<FontSet>) -> Result<Renderer, String> {
        unsafe {
            let program = build_program(&gl)?;
            let vao_grid = gl.create_vertex_array()?;
            let vbo_grid = gl.create_buffer()?;
            let vao_overlay = gl.create_vertex_array()?;
            let vbo_overlay = gl.create_buffer()?;
            let vbo_quad = gl.create_buffer()?;

            let quad: [f32; 24] = [
                0.0, 0.0, 0.0, 0.0, //
                1.0, 0.0, 1.0, 0.0, //
                0.0, 1.0, 0.0, 1.0, //
                0.0, 1.0, 0.0, 1.0, //
                1.0, 0.0, 1.0, 0.0, //
                1.0, 1.0, 1.0, 1.0, //
            ];
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo_quad));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(&quad), glow::STATIC_DRAW);

            configure_vao(&gl, vao_grid, vbo_grid, vbo_quad);
            configure_vao(&gl, vao_overlay, vbo_overlay, vbo_quad);

            let u_res = gl.get_uniform_location(program, "u_res");
            let u_atlas = gl.get_uniform_location(program, "u_atlas");
            let atlas = Atlas::new(&gl)?;

            Ok(Renderer {
                gl,
                program,
                u_res,
                u_atlas,
                vao_grid,
                vbo_grid,
                vao_overlay,
                vbo_overlay,
                vbo_quad,
                atlas,
                fonts,
                width: 1,
                height: 1,
                grid_capacity: 0,
                grid_count: 0,
                grid_cols: 0,
            })
        }
    }

    pub fn debug_counts(&self) -> (usize, usize) {
        (self.grid_capacity, self.grid_count)
    }

    #[inline]
    pub fn cell_w(&self) -> f32 {
        self.fonts.cell_w
    }

    #[inline]
    pub fn cell_h(&self) -> f32 {
        self.fonts.cell_h
    }

    pub fn glyph(&mut self, ch: char, style: u8) -> Option<Glyph> {
        let atlas = &mut self.atlas;
        let fonts = &self.fonts;
        atlas.glyph(&self.gl, fonts, ch, style)
    }

    /// Prepara il buffer della griglia per `cols`×`rows` celle (3 istanze per cella).
    /// Restituisce `true` se il buffer è stato rifatto (serve un ridisegno completo).
    pub fn reserve_grid(&mut self, cols: usize, rows: usize) -> bool {
        let need = cols.max(1) * rows.max(1) * INST_PER_CELL;
        if need == self.grid_capacity {
            return false;
        }
        let bytes = need * std::mem::size_of::<Instance>();
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo_grid));
            let zeros = vec![0u8; bytes];
            self.gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &zeros, glow::DYNAMIC_DRAW);
        }
        self.grid_capacity = need;
        self.grid_count = need;
        self.grid_cols = cols.max(1);
        true
    }

    /// Cambia font a caldo: atlante nuovo, griglia da riscrivere tutta.
    pub fn set_fonts(&mut self, fonts: Rc<FontSet>) {
        self.fonts = fonts;
        self.atlas.clear(&self.gl);
        let fonts = self.fonts.clone();
        self.atlas.prewarm(&self.gl, &fonts);
        self.grid_capacity = 0;
        self.grid_count = 0;
        self.grid_cols = 0;
    }

    /// Riscrive una riga intera del buffer.
    pub fn upload_row(&mut self, row: usize, cols: usize, data: &[Instance]) {
        let per_row = self.grid_cols * INST_PER_CELL;
        let offset = row * per_row;
        if offset + data.len() > self.grid_capacity || data.len() > cols * INST_PER_CELL {
            return;
        }
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo_grid));
            self.gl.buffer_sub_data_u8_slice(
                glow::ARRAY_BUFFER,
                (offset * std::mem::size_of::<Instance>()) as i32,
                as_bytes(data),
            );
        }
    }

    /// Carica più righe consecutive con **una sola** chiamata: il disegno di un
    /// fotogramma intero passa da 32 `buffer_sub_data` a una.
    pub fn upload_span(&mut self, first_row: usize, data: &[Instance]) {
        let per_row = self.grid_cols * INST_PER_CELL;
        if per_row == 0 || data.is_empty() {
            return;
        }
        let offset = first_row * per_row;
        if offset + data.len() > self.grid_capacity {
            return;
        }
        unsafe {
            self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo_grid));
            self.gl.buffer_sub_data_u8_slice(
                glow::ARRAY_BUFFER,
                (offset * std::mem::size_of::<Instance>()) as i32,
                as_bytes(data),
            );
        }
    }

    pub fn set_size(&mut self, w: i32, h: i32) {
        self.width = w.max(1);
        self.height = h.max(1);
    }

    pub fn draw(&mut self, clear: Rgb, overlay: &[Instance]) {
        unsafe {
            let gl = &self.gl;
            gl.viewport(0, 0, self.width, self.height);
            let [r, g, b, _] = clear.rgba();
            gl.clear_color(r, g, b, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::DEPTH_TEST);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
            gl.use_program(Some(self.program));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(self.atlas.texture()));
            if let Some(loc) = self.u_atlas {
                gl.uniform_1_i32(Some(&loc), 0);
            }
            if let Some(loc) = self.u_res {
                gl.uniform_2_f32(Some(&loc), self.width as f32, self.height as f32);
            }

            if self.grid_count > 0 {
                gl.bind_vertex_array(Some(self.vao_grid));
                gl.draw_arrays_instanced(glow::TRIANGLES, 0, 6, self.grid_count as i32);
            }
            if !overlay.is_empty() {
                gl.bind_vertex_array(Some(self.vao_overlay));
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo_overlay));
                gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, as_bytes(overlay), glow::STREAM_DRAW);
                gl.draw_arrays_instanced(glow::TRIANGLES, 0, 6, overlay.len() as i32);
            }
            gl.bind_vertex_array(None);
        }
    }

    /// Istanze di una riga di terminale: sfondo, glifo, decorazione (3 per cella).
    #[allow(clippy::too_many_arguments)]
    pub fn row_instances(
        &mut self,
        cells: &[Cell],
        x0: f32,
        y0: f32,
        sel: Option<(usize, usize)>,
        cursor: Option<usize>,
        sel_color: Rgb,
        _bg_default: Rgb,
        out: &mut Vec<Instance>,
    ) {
        // NB: NON si azzera `out` qui: dalla 0.3.0 il chiamante accumula **più righe**
        // nello stesso buffer e poi le carica con una sola `upload_span`.  Con la
        // `out.clear()` qui dentro restava solo l'ultima riga (misurato: 650 istanze
        // invece di 20.800) e il terminale sembrava vuoto.
        let cw = self.fonts.cell_w;
        let ch_ = self.fonts.cell_h;
        let asc = self.fonts.ascent;
        let white = self.atlas.white;
        let under_y = asc + (ch_ - asc) * 0.5;
        let under_h = self.fonts.underline_size.max(1.0);

        for (col, cell) in cells.iter().enumerate() {
            let mut fg = cell.fg;
            let mut bg = cell.bg;
            if cell.flags & flags::INVERSE != 0 {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags & flags::DIM != 0 {
                fg = fg.mix(bg, 0.55);
            }
            if cell.flags & flags::HIDDEN != 0 {
                fg = bg;
            }
            if sel.map(|(a, b)| col >= a && col <= b).unwrap_or(false) {
                bg = bg.mix(sel_color, 0.75);
            }
            if cursor == Some(col) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let x = x0 + col as f32 * cw;

            let th_base = (ch_ / 10.0).round().max(1.0);
            let mut gfx = [Instance::default(); INST_PER_CELL - 1];
            let mut n_gfx = 0usize;

            if let Some((l, r, u, d, fat)) = box_parts(cell.ch) {
                let th = (th_base * fat).max(1.0);
                let gap = th;
                if l > 0 || r > 0 {
                    let xa = if l > 0 { x } else { x + cw * 0.5 };
                    let xb = if r > 0 { x + cw } else { x + cw * 0.5 };
                    if l.max(r) >= 2 {
                        let total = th * 2.0 + gap;
                        let yt = y0 + (ch_ - total) * 0.5;
                        gfx[n_gfx] = Instance::solid(xa, yt, xb - xa, th, fg, white);
                        gfx[n_gfx + 1] = Instance::solid(xa, yt + th + gap, xb - xa, th, fg, white);
                        n_gfx += 2;
                    } else {
                        gfx[n_gfx] = Instance::solid(xa, y0 + (ch_ - th) * 0.5, xb - xa, th, fg, white);
                        n_gfx += 1;
                    }
                }
                if u > 0 || d > 0 {
                    let ya = if u > 0 { y0 } else { y0 + ch_ * 0.5 };
                    let yb = if d > 0 { y0 + ch_ } else { y0 + ch_ * 0.5 };
                    if u.max(d) >= 2 {
                        let total = th * 2.0 + gap;
                        let xt = x + (cw - total) * 0.5;
                        gfx[n_gfx] = Instance::solid(xt, ya, th, yb - ya, fg, white);
                        gfx[n_gfx + 1] = Instance::solid(xt + th + gap, ya, th, yb - ya, fg, white);
                        n_gfx += 2;
                    } else {
                        gfx[n_gfx] = Instance::solid(x + (cw - th) * 0.5, ya, th, yb - ya, fg, white);
                        n_gfx += 1;
                    }
                }
            } else if let Some((bx0, by0, bx1, by1, dens)) = block_rect(cell.ch) {
                let col = if dens >= 255 { fg } else { bg.mix(fg, dens as f32 / 255.0) };
                gfx[n_gfx] = Instance::solid(
                    x + cw * bx0,
                    y0 + ch_ * by0,
                    cw * (bx1 - bx0),
                    ch_ * (by1 - by0),
                    col,
                    white,
                );
                n_gfx += 1;
            } else if !cell.is_spacer() && cell.ch != ' ' {
                let style = (if cell.flags & flags::BOLD != 0 { STYLE_BOLD } else { 0 })
                    | (if cell.flags & flags::ITALIC != 0 { STYLE_ITALIC } else { 0 });
                if let Some(g) = self.glyph(cell.ch, style) {
                    let [r_, g_, b_, a_] = fg.rgba();
                    gfx[n_gfx] = Instance {
                        x: (x + g.left).round(),
                        y: (y0 + asc - g.top).round(),
                        w: g.w,
                        h: g.h,
                        r: r_,
                        g: g_,
                        b: b_,
                        a: a_,
                        u0: g.u0,
                        v0: g.v0,
                        u1: g.u1,
                        v1: g.v1,
                    };
                    n_gfx += 1;
                }
            }

            // Sottolineatura o barratura, se c'è ancora posto.
            if n_gfx < INST_PER_CELL - 1 {
                if cell.flags & flags::UNDERLINE != 0 {
                    gfx[n_gfx] = Instance::solid(x, y0 + under_y, cw, under_h, fg, white);
                    n_gfx += 1;
                } else if cell.flags & flags::STRIKE != 0 {
                    gfx[n_gfx] = Instance::solid(x, y0 + asc * 0.55, cw, under_h, fg, white);
                    n_gfx += 1;
                }
            }

            out.push(Instance::solid(x, y0, cw, ch_, bg, white));
            out.extend_from_slice(&gfx[..]);
        }
    }

    pub fn ui_rect(&self, out: &mut Vec<Instance>, x: f32, y: f32, w: f32, h: f32, color: Rgb) {
        out.push(Instance::solid(x, y, w, h, color, self.atlas.white));
    }

    /// Copia il fotogramma in un file PPM (autocollaudo senza schermo).
    pub fn write_ppm(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let (w, h) = (self.width, self.height);
        let mut buf = vec![0u8; (w * h * 4) as usize];
        unsafe {
            self.gl.read_pixels(
                0,
                0,
                w,
                h,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut buf)),
            );
        }
        let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
        write!(out, "P6\n{w} {h}\n255\n")?;
        for y in (0..h).rev() {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                out.write_all(&buf[i..i + 3])?;
            }
        }
        out.flush()
    }
}

fn as_bytes<T: Copy>(slice: &[T]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice)) }
}

unsafe fn configure_vao(
    gl: &glow::Context,
    vao: glow::VertexArray,
    vbo_instances: glow::Buffer,
    vbo_quad: glow::Buffer,
) {
    unsafe {
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo_quad));
        gl.enable_vertex_attrib_array(0);
        gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(1);
        gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);

        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo_instances));
        let stride = std::mem::size_of::<Instance>() as i32;
        gl.enable_vertex_attrib_array(2);
        gl.vertex_attrib_pointer_f32(2, 2, glow::FLOAT, false, stride, 0);
        gl.vertex_attrib_divisor(2, 1);
        gl.enable_vertex_attrib_array(3);
        gl.vertex_attrib_pointer_f32(3, 2, glow::FLOAT, false, stride, 8);
        gl.vertex_attrib_divisor(3, 1);
        gl.enable_vertex_attrib_array(4);
        gl.vertex_attrib_pointer_f32(4, 4, glow::FLOAT, false, stride, 16);
        gl.vertex_attrib_divisor(4, 1);
        gl.enable_vertex_attrib_array(5);
        gl.vertex_attrib_pointer_f32(5, 4, glow::FLOAT, false, stride, 32);
        gl.vertex_attrib_divisor(5, 1);
        gl.bind_vertex_array(None);
    }
}

unsafe fn build_program(gl: &glow::Context) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        let mut shaders = Vec::new();
        for (kind, src) in [(glow::VERTEX_SHADER, VERTEX), (glow::FRAGMENT_SHADER, FRAGMENT)] {
            let shader = gl.create_shader(kind)?;
            gl.shader_source(shader, src);
            gl.compile_shader(shader);
            if !gl.get_shader_compile_status(shader) {
                return Err(format!("shader: {}", gl.get_shader_info_log(shader)));
            }
            gl.attach_shader(program, shader);
            shaders.push(shader);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(format!("link: {}", gl.get_program_info_log(program)));
        }
        for s in shaders {
            gl.detach_shader(program, s);
            gl.delete_shader(s);
        }
        Ok(program)
    }
}
