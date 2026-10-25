//! Scelta e misure del font monospazio di sistema (via `fc-match`, senza dipendenze grafiche).

use std::process::Command;

use swash::FontRef;

pub const STYLE_BOLD: u8 = 1;
pub const STYLE_ITALIC: u8 = 2;

pub struct FontSet {
    pub family: String,
    pub path: String,
    regular: Vec<u8>,
    bold: Option<Vec<u8>>,
    italic: Option<Vec<u8>>,
    /// Font di riserva per i simboli mancanti (riquadri, frecce, emoji semplici).
    fallback: Option<Vec<u8>>,
    pub size: f32,
    pub cell_w: f32,
    pub cell_h: f32,
    pub ascent: f32,
    pub underline_offset: f32,
    pub underline_size: f32,
    pub bold_strength: f32,
}

fn fc_match(pattern: &str, field: &str) -> Option<String> {
    let out = Command::new("fc-match")
        .args(["-f", field, pattern])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

fn read_file(path: &str) -> Option<Vec<u8>> {
    std::fs::read(path).ok().filter(|d| !d.is_empty())
}

impl FontSet {
    pub fn load(
        family: Option<&str>,
        size: f32,
        line_height: f32,
        bold_strength: f32,
    ) -> Result<FontSet, String> {
        let pattern = match family {
            Some(f) if !f.is_empty() => format!("{f}:spacing=mono"),
            _ => "monospace:spacing=mono".to_string(),
        };
        let mut file = fc_match(&pattern, "%{file}");
        let mut family_name = fc_match(&pattern, "%{family}").unwrap_or_else(|| "monospace".into());

        if file.is_none() || read_file(file.as_deref().unwrap_or("")).is_none() {
            file = fc_match("monospace", "%{file}");
            family_name = fc_match("monospace", "%{family}").unwrap_or_else(|| "monospace".into());
        }
        let path = file.ok_or_else(|| "nessun font monospazio trovato (manca fontconfig?)".to_string())?;
        let regular = read_file(&path).ok_or_else(|| format!("font illeggibile: {path}"))?;

        // Il carattere in neretto/corsivo si cerca per famiglia: se il file è lo stesso,
        // il font non li ha e si usa il grassetto finto.
        let family_name = family_name.split(',').next().unwrap_or("monospace").trim().to_string();
        let bold = fc_match(&format!("{family_name}:style=Bold"), "%{file}")
            .filter(|p| p != &path)
            .and_then(|p| read_file(&p).map(|d| (p, d)));
        let italic = fc_match(&format!("{family_name}:style=Italic"), "%{file}")
            .filter(|p| p != &path)
            .and_then(|p| read_file(&p).map(|d| (p, d)));

        let mut fs = FontSet {
            family: family_name,
            path,
            regular,
            bold: bold.map(|(_, d)| d),
            italic: italic.map(|(_, d)| d),
            fallback: None,
            size,
            cell_w: size * 0.6,
            cell_h: (size * line_height).round(),
            ascent: size * 0.8,
            underline_offset: 0.0,
            underline_size: 1.0,
            bold_strength,
        };
        // Riserva: un monospazio con molti simboli (DejaVu ha i riquadri di ncurses).
        if let Some(fb) = fc_match("DejaVu Sans Mono:spacing=mono", "%{file}") {
            if fb != fs.path {
                if let Some(data) = read_file(&fb) {
                    fs.fallback = Some(data);
                }
            }
        }
        fs.measure(line_height)?;
        Ok(fs)
    }

    fn measure(&mut self, line_height: f32) -> Result<(), String> {
        let font = FontRef::from_index(&self.regular, 0).ok_or("font non leggibile (indice 0)")?;
        let m = font.metrics(&[]);
        let upem = m.units_per_em as f32;
        if upem <= 0.0 {
            return Err("font senza unità em".into());
        }
        let scale = self.size / upem;
        let advance = if m.average_width > 0.0 { m.average_width } else { m.max_width };
        self.cell_w = (advance * scale).round().max(1.0);
        let ascent = (m.ascent * scale).round();
        let descent = (m.descent.abs() * scale).round();
        self.ascent = ascent;
        self.cell_h = ((ascent + descent) * line_height).round().max(ascent + 1.0);
        self.underline_offset = (ascent - m.strikeout_offset * scale * -1.0).max(ascent - 2.0);
        self.underline_size = (m.stroke_size * scale).round().max(1.0);
        Ok(())
    }

    /// Byte del font per lo stile richiesto (nero = regolare).
    pub fn data_for(&self, style: u8) -> (&[u8], bool, bool) {
        let bold = style & STYLE_BOLD != 0;
        let italic = style & STYLE_ITALIC != 0;
        if italic && bold {
            if let (Some(b), Some(i)) = (&self.bold, &self.italic) {
                let _ = (b, i);
            }
        }
        if bold {
            if let Some(b) = &self.bold {
                return (b, false, italic);
            }
            return (&self.regular, true, italic);
        }
        if italic {
            if let Some(i) = &self.italic {
                return (i, false, false);
            }
            return (&self.regular, false, true);
        }
        (&self.regular, false, false)
    }

    pub fn fallback_data(&self) -> Option<&[u8]> {
        self.fallback.as_deref()
    }

    pub fn has_bold_face(&self) -> bool {
        self.bold.is_some()
    }

    pub fn info(&self) -> String {
        format!(
            "font: {} ({})\nsize: {:.1}px\ncella: {:.0}x{:.0}px\nascent: {:.0}px\nneretto: {}",
            self.family,
            self.path,
            self.size,
            self.cell_w,
            self.cell_h,
            self.ascent,
            if self.has_bold_face() { "sì" } else { "finto" }
        )
    }
}
