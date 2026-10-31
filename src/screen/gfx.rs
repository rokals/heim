//! Un dessin minimal en mémoire (RGB565, le format des petits écrans SPI) : rectangles
//! arrondis et texte (police DejaVu Sans Bold, rendue par fontdue).

use std::collections::HashMap;

pub const FONT: &[u8] = include_bytes!("../../assets/fonts/DejaVuSans-Bold.ttf");

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn hex(s: &str) -> Rgb {
        let p = |i: usize| {
            u8::from_str_radix(s.trim_start_matches('#').get(i..i + 2).unwrap_or("00"), 16)
                .unwrap_or(0)
        };
        Rgb(p(0), p(2), p(4))
    }

    fn to565(self) -> u16 {
        ((self.0 as u16 & 0xF8) << 8) | ((self.1 as u16 & 0xFC) << 3) | (self.2 as u16 >> 3)
    }

    fn from565(v: u16) -> Rgb {
        let r = ((v >> 11) & 0x1F) as u8;
        let g = ((v >> 5) & 0x3F) as u8;
        let b = (v & 0x1F) as u8;
        Rgb(r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2)
    }

    fn blend(self, over: Rgb, a: f32) -> Rgb {
        let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * a).round() as u8;
        Rgb(m(self.0, over.0), m(self.1, over.1), m(self.2, over.2))
    }
}

pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u16>,
    font: fontdue::Font,
    glyphs: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        let font = fontdue::Font::from_bytes(FONT, fontdue::FontSettings::default())
            .expect("police intégrée");
        Canvas {
            w,
            h,
            px: vec![0; (w * h) as usize],
            font,
            glyphs: HashMap::new(),
        }
    }

    pub fn clear(&mut self, c: Rgb) {
        let v = c.to565();
        self.px.iter_mut().for_each(|p| *p = v);
    }

    fn put(&mut self, x: i32, y: i32, c: Rgb, a: f32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || a <= 0.0 {
            return;
        }
        let i = (y * self.w + x) as usize;
        self.px[i] = if a >= 1.0 {
            c.to565()
        } else {
            Rgb::from565(self.px[i]).blend(c, a).to565()
        };
    }

    /// Rectangle aux coins arrondis (rayon `r`), bords lissés.
    pub fn round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: Rgb) {
        let r = r.min(w / 2).min(h / 2).max(0);
        for yy in y..y + h {
            for xx in x..x + w {
                let cx = if xx < x + r {
                    x + r
                } else if xx >= x + w - r {
                    x + w - r - 1
                } else {
                    xx
                };
                let cy = if yy < y + r {
                    y + r
                } else if yy >= y + h - r {
                    y + h - r - 1
                } else {
                    yy
                };
                let a = if cx == xx || cy == yy {
                    1.0
                } else {
                    let d = (((xx - cx) * (xx - cx) + (yy - cy) * (yy - cy)) as f32).sqrt();
                    (r as f32 + 0.5 - d).clamp(0.0, 1.0)
                };
                self.put(xx, yy, c, a);
            }
        }
    }

    pub fn circle(&mut self, cx: i32, cy: i32, r: i32, c: Rgb) {
        self.round_rect(cx - r, cy - r, 2 * r, 2 * r, r, c);
    }

    fn glyph(&mut self, ch: char, size: u32) -> (fontdue::Metrics, Vec<u8>) {
        if let Some(g) = self.glyphs.get(&(ch, size)) {
            return g.clone();
        }
        let g = self.font.rasterize(ch, size as f32);
        self.glyphs.insert((ch, size), g.clone());
        g
    }

    /// Largeur d'un texte, en pixels.
    pub fn text_width(&mut self, s: &str, size: u32) -> i32 {
        s.chars()
            .map(|c| self.glyph(c, size).0.advance_width.round() as i32)
            .sum()
    }

    /// Texte posé sur sa ligne de base (`y`), coupé avec « … » s'il dépasse `max_w`.
    pub fn text(&mut self, x: i32, y: i32, s: &str, size: u32, c: Rgb, max_w: i32) {
        let mut shown: String = s.to_string();
        if self.text_width(&shown, size) > max_w {
            while !shown.is_empty() && self.text_width(&format!("{shown}…"), size) > max_w {
                shown.pop();
            }
            shown.push('…');
        }
        let mut pen = x;
        for ch in shown.chars() {
            let (m, bmp) = self.glyph(ch, size);
            let gx = pen + m.xmin;
            let gy = y - m.height as i32 - m.ymin;
            for row in 0..m.height {
                for col in 0..m.width {
                    let a = bmp[row * m.width + col] as f32 / 255.0;
                    self.put(gx + col as i32, gy + row as i32, c, a);
                }
            }
            pen += m.advance_width.round() as i32;
        }
    }

    /// Texte centré horizontalement dans [x, x + w].
    pub fn text_center(&mut self, x: i32, w: i32, y: i32, s: &str, size: u32, c: Rgb) {
        let tw = self.text_width(s, size).min(w);
        self.text(x + (w - tw) / 2, y, s, size, c, w);
    }

    /// Coupe un texte en deux lignes au plus, selon la largeur.
    pub fn wrap2(&mut self, s: &str, size: u32, max_w: i32) -> (String, Option<String>) {
        if self.text_width(s, size) <= max_w {
            return (s.to_string(), None);
        }
        let words: Vec<&str> = s.split_whitespace().collect();
        let mut first = String::new();
        let mut i = 0;
        while i < words.len() {
            let candidate = if first.is_empty() {
                words[i].to_string()
            } else {
                format!("{first} {}", words[i])
            };
            if self.text_width(&candidate, size) > max_w && !first.is_empty() {
                break;
            }
            first = candidate;
            i += 1;
        }
        let rest = words[i..].join(" ");
        (first, (!rest.is_empty()).then_some(rest))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_text_and_shapes() {
        let mut c = Canvas::new(120, 40);
        c.clear(Rgb(0, 0, 0));
        c.round_rect(2, 2, 50, 30, 8, Rgb(255, 255, 255));
        assert_eq!(c.px[(15 * 120 + 20) as usize], 0xFFFF);
        assert_eq!(c.px[0], 0); // coin arrondi
        c.text(60, 30, "Salon", 16, Rgb(255, 255, 255), 60);
        assert!(c.px[60..].iter().any(|&p| p != 0));
        let (a, b) = c.wrap2("Lampe du salon côté fenêtre", 16, 100);
        assert!(!a.is_empty() && b.is_some());
        assert_eq!(Rgb::hex("#ff8800"), Rgb(255, 136, 0));
    }
}
