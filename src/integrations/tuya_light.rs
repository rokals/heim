//! Lampes Tuya : couleur, blanc, luminosité, modes.
//!
//! La couleur d'une lampe Tuya tient dans un seul point de données (`colour_data` ou
//! `colour_data_v2`), sous trois formes selon les modèles :
//! - v2 : 12 caractères hexadécimaux `HHHHSSSSVVVV` (teinte 0-360, saturation et valeur 0-1000) ;
//! - v1 : 14 caractères `RRGGBBHHHHSSVV` (rouge, vert, bleu, puis teinte 0-360, saturation et
//!   valeur 0-255) ;
//! - JSON : `{"h":120,"s":1000,"v":1000}`.
//!
//! En mode couleur (`work_mode` = `colour`), la luminosité est la « valeur » de la couleur, pas
//! `bright_value` : on la lit et on la règle là.

use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    V1,
    V2,
    Json,
}

/// Teinte en degrés, saturation et valeur de 0 à 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Colour {
    pub h: f64,
    pub s: f64,
    pub v: f64,
}

/// Le format d'après la description du cloud (`{"h":{"max":360},"s":{"max":1000}…}`) et le
/// nom du point de données.
pub fn format_of(code: &str, values: &Value) -> Format {
    let smax = values
        .pointer("/s/max")
        .and_then(Value::as_f64)
        .unwrap_or(if code.ends_with("_v2") { 1000.0 } else { 255.0 });
    if smax > 255.0 { Format::V2 } else { Format::V1 }
}

/// Lit une couleur reçue, quel que soit son format. Renvoie aussi le format vu.
pub fn decode(value: &Value) -> Option<(Colour, Format)> {
    let s = value.as_str()?.trim();
    if s.starts_with('{') {
        let v: Value = serde_json::from_str(s).ok()?;
        let n = |k: &str| v.get(k).and_then(Value::as_f64);
        let (h, sat, val) = (n("h")?, n("s")?, n("v")?);
        // Le JSON suit l'échelle v2 (0-1000) ; certains modèles anciens envoient 0-255.
        let scale = if sat > 255.0 || val > 255.0 {
            1000.0
        } else {
            255.0
        };
        return Some((
            Colour {
                h: h.rem_euclid(360.0),
                s: (sat / scale).clamp(0.0, 1.0),
                v: (val / scale).clamp(0.0, 1.0),
            },
            Format::Json,
        ));
    }
    let hex = |a: usize, b: usize| u32::from_str_radix(s.get(a..b)?, 16).ok().map(f64::from);
    match s.len() {
        12 => Some((
            Colour {
                h: hex(0, 4)?.rem_euclid(360.0),
                s: (hex(4, 8)? / 1000.0).clamp(0.0, 1.0),
                v: (hex(8, 12)? / 1000.0).clamp(0.0, 1.0),
            },
            Format::V2,
        )),
        14 => Some((
            Colour {
                h: hex(6, 10)?.rem_euclid(360.0),
                s: (hex(10, 12)? / 255.0).clamp(0.0, 1.0),
                v: (hex(12, 14)? / 255.0).clamp(0.0, 1.0),
            },
            Format::V1,
        )),
        _ => None,
    }
}

/// Écrit une couleur dans le format de l'appareil.
pub fn encode(c: Colour, f: Format) -> Value {
    let h = c.h.rem_euclid(360.0).round() as u32;
    match f {
        Format::V2 => json!(format!(
            "{h:04x}{:04x}{:04x}",
            (c.s * 1000.0).round() as u32,
            (c.v * 1000.0).round().max(10.0) as u32
        )),
        Format::Json => json!(
            json!({
                "h": h,
                "s": (c.s * 1000.0).round() as u32,
                "v": (c.v * 1000.0).round().max(10.0) as u32
            })
            .to_string()
        ),
        Format::V1 => {
            let (r, g, b) = hsv_to_rgb(c);
            json!(format!(
                "{r:02x}{g:02x}{b:02x}{h:04x}{:02x}{:02x}",
                (c.s * 255.0).round() as u32,
                (c.v * 255.0).round().max(3.0) as u32
            ))
        }
    }
}

fn hsv_to_rgb(c: Colour) -> (u8, u8, u8) {
    let h = c.h.rem_euclid(360.0) / 60.0;
    let chroma = c.v * c.s;
    let x = chroma * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = c.v - chroma;
    let to = |u: f64| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

/// `#rrggbb` → teinte (degrés) et saturation (0-1).
pub fn hex_to_hs(hex: &str) -> Option<(f64, f64)> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let c = |i: usize| {
        u8::from_str_radix(&s[i..i + 2], 16)
            .ok()
            .map(|v| v as f64 / 255.0)
    };
    let (r, g, b) = (c(0)?, c(2)?, c(4)?);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    Some((h, s))
}

/// Ce que l'on demande à une lampe (d'une commande Heim ou de la fiche de l'appareil).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Intent {
    pub on: Option<bool>,
    /// 1..=255
    pub brightness: Option<u8>,
    /// En mireds.
    pub color_temp: Option<u16>,
    /// Teinte (degrés), saturation (0-1).
    pub hs: Option<(f64, f64)>,
    pub effect: Option<String>,
}

impl Intent {
    /// Les réglages de la fiche (`state`, `brightness`, `color`, `color_temp`, `effect`) ; les
    /// autres clés restent pour les points de données bruts.
    pub fn take_from(values: &mut Map<String, Value>) -> Option<Intent> {
        let mut i = Intent::default();
        let mut any = false;
        if let Some(v) = values.remove("state") {
            any = true;
            i.on = match v.as_str().map(str::to_ascii_uppercase).as_deref() {
                Some("ON") => Some(true),
                Some("OFF") => Some(false),
                Some("TOGGLE") => None,
                _ => v.as_bool(),
            };
        }
        if let Some(v) = values.remove("brightness") {
            any = true;
            i.brightness = v.as_f64().map(|b| b.clamp(1.0, 255.0) as u8);
        }
        if let Some(v) = values.remove("color_temp") {
            any = true;
            i.color_temp = v.as_f64().map(|t| t.clamp(153.0, 500.0) as u16);
        }
        if let Some(v) = values.remove("color") {
            any = true;
            i.hs = if let Some(hex) = v.get("hex").and_then(Value::as_str).or(v.as_str()) {
                hex_to_hs(hex)
            } else {
                let h = v.get("hue").or_else(|| v.get("h")).and_then(Value::as_f64);
                let s = v
                    .get("saturation")
                    .or_else(|| v.get("s"))
                    .and_then(Value::as_f64);
                h.zip(s)
                    .map(|(h, s)| (h, if s > 1.0 { s / 100.0 } else { s }))
            };
        }
        if let Some(v) = values.remove("effect") {
            any = true;
            i.effect = v.as_str().map(str::to_string);
        }
        values.remove("transition");
        any.then_some(i)
    }
}

/// Le contexte d'une lampe : ses points de données.
#[derive(Debug, Clone, PartialEq)]
pub struct Lamp {
    pub switch: String,
    pub bright: Option<(String, f64, f64)>,
    pub temp: Option<(String, f64, f64)>,
    pub mode: Option<String>,
    pub colour: Option<(String, Format)>,
}

impl Lamp {
    fn in_colour(&self, last: &Map<String, Value>) -> bool {
        self.mode
            .as_ref()
            .and_then(|m| last.get(m))
            .and_then(Value::as_str)
            == Some("colour")
    }

    /// La couleur actuelle (dernier état reçu).
    pub fn colour_now(&self, last: &Map<String, Value>) -> Option<Colour> {
        let (dp, _) = self.colour.as_ref()?;
        decode(last.get(dp)?).map(|(c, _)| c)
    }

    /// Les points de données à envoyer. `seen` : le format de couleur que l'appareil utilise
    /// vraiment (lu dans ses messages), qui l'emporte sur celui annoncé par le cloud ; était :
    /// JSON, on lui répond pareil.
    pub fn dps(
        &self,
        i: &Intent,
        last: &Map<String, Value>,
        seen: Option<Format>,
    ) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(on) = i.on {
            out.insert(self.switch.clone(), json!(on));
        }
        let wants_light = i.brightness.is_some() || i.color_temp.is_some() || i.hs.is_some();
        if wants_light && i.on.is_none() {
            out.insert(self.switch.clone(), json!(true));
        }
        if i.on == Some(false) {
            return out;
        }
        let set_mode = |out: &mut Map<String, Value>, m: &str| {
            if let Some(dp) = &self.mode {
                out.insert(dp.clone(), json!(m));
            }
        };
        // Couleur demandée, ou luminosité seule alors que la lampe est en couleur.
        let colour_target = i.hs.is_some()
            || (i.brightness.is_some() && i.color_temp.is_none() && self.in_colour(last))
            || (i.brightness.is_some() && self.bright.is_none() && self.colour.is_some());
        if colour_target && let Some((dp, f)) = &self.colour {
            let now = self.colour_now(last).unwrap_or(Colour {
                h: 0.0,
                s: 1.0,
                v: 1.0,
            });
            let (h, s) = i.hs.unwrap_or((now.h, now.s));
            let v = i
                .brightness
                .map(|b| b as f64 / 255.0)
                .unwrap_or(if now.v > 0.0 { now.v } else { 1.0 });
            let f = seen.unwrap_or(*f);
            out.insert(dp.clone(), encode(Colour { h, s, v }, f));
            set_mode(&mut out, "colour");
            return out;
        }
        let mut white = false;
        if let (Some(b), Some((dp, min, max))) = (i.brightness, &self.bright) {
            let v = min + (b.max(1) as f64 - 1.0) / 254.0 * (max - min);
            out.insert(dp.clone(), json!(v.round() as i64));
            white = true;
        }
        if let (Some(ct), Some((dp, min, max))) = (i.color_temp, &self.temp) {
            let ratio = ((500.0 - ct as f64) / 347.0).clamp(0.0, 1.0);
            out.insert(
                dp.clone(),
                json!((min + ratio * (max - min)).round() as i64),
            );
            white = true;
        }
        if white {
            set_mode(&mut out, "white");
        }
        if let Some(e) = &i.effect {
            set_mode(&mut out, e);
        }
        out
    }

    /// Luminosité (1-255) et mode de couleur d'après le dernier état.
    pub fn brightness_now(&self, last: &Map<String, Value>) -> Option<u8> {
        if self.in_colour(last) || self.bright.is_none() {
            return self
                .colour_now(last)
                .map(|c| (c.v * 254.0 + 1.0).round().clamp(1.0, 255.0) as u8);
        }
        let (dp, min, max) = self.bright.as_ref()?;
        let v = last.get(dp).and_then(Value::as_f64)?;
        Some(
            ((v - min) / (max - min) * 254.0 + 1.0)
                .round()
                .clamp(1.0, 255.0) as u8,
        )
    }

    /// Ce que la fiche de l'appareil affiche : couleur actuelle, mode, effet.
    pub fn values(&self, last: &Map<String, Value>) -> Map<String, Value> {
        let mut out = Map::new();
        if let Some(c) = self.colour_now(last) {
            out.insert(
                "color".into(),
                json!({ "hue": c.h.round(), "saturation": (c.s * 100.0).round() }),
            );
        }
        let mode = self
            .mode
            .as_ref()
            .and_then(|m| last.get(m))
            .and_then(Value::as_str);
        if let Some(m) = mode {
            out.insert(
                "color_mode".into(),
                json!(match m {
                    "colour" => "hs",
                    "white" => "color_temp",
                    other => other,
                }),
            );
            if m != "white" && m != "colour" {
                out.insert("effect".into(), json!(m));
            }
        }
        if let Some(b) = self.brightness_now(last) {
            out.insert("brightness".into(), json!(b));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp() -> Lamp {
        Lamp {
            switch: "20".into(),
            bright: Some(("22".into(), 10.0, 1000.0)),
            temp: Some(("23".into(), 0.0, 1000.0)),
            mode: Some("21".into()),
            colour: Some(("24".into(), Format::V2)),
        }
    }

    #[test]
    fn colours_roundtrip_in_every_format() {
        let red = Colour {
            h: 0.0,
            s: 1.0,
            v: 1.0,
        };
        assert_eq!(encode(red, Format::V2), json!("000003e803e8"));
        assert_eq!(decode(&json!("000003e803e8")).unwrap(), (red, Format::V2));
        let v1 = encode(red, Format::V1);
        assert_eq!(v1, json!("ff00000000ffff"));
        assert_eq!(decode(&v1).unwrap().1, Format::V1);
        let js = encode(
            Colour {
                h: 120.0,
                s: 0.5,
                v: 1.0,
            },
            Format::Json,
        );
        let (c, f) = decode(&js).unwrap();
        assert_eq!(f, Format::Json);
        assert_eq!((c.h, c.s), (120.0, 0.5));
        assert_eq!(hex_to_hs("#00ff00"), Some((120.0, 1.0)));
    }

    #[test]
    fn colour_and_brightness_go_to_the_right_place() {
        let l = lamp();
        let mut v = Map::new();
        v.insert("state".into(), json!("ON"));
        v.insert("color".into(), json!({ "hex": "#0000ff" }));
        v.insert("transition".into(), json!(2));
        v.insert("102".into(), json!(5));
        let i = Intent::take_from(&mut v).unwrap();
        assert_eq!(v.len(), 1, "le point de données brut reste");
        let out = l.dps(&i, &Map::new(), None);
        assert_eq!(out["20"], json!(true));
        assert_eq!(out["21"], json!("colour"));
        assert_eq!(out["24"], json!("00f003e803e8"));

        // En couleur, la luminosité seule change la valeur de la couleur.
        let mut last = Map::new();
        last.insert("21".into(), json!("colour"));
        last.insert("24".into(), json!("00f003e803e8"));
        let dim = Intent {
            brightness: Some(128),
            ..Default::default()
        };
        let out = l.dps(&dim, &last, None);
        assert_eq!(out["24"], json!("00f003e801f6"));
        assert!(!out.contains_key("22"));
        assert_eq!(
            l.values(&last)["color"],
            json!({ "hue": 240.0, "saturation": 100.0 })
        );
        assert_eq!(l.values(&last)["color_mode"], json!("hs"));

        // Le blanc remet le mode blanc.
        let warm = Intent {
            color_temp: Some(500),
            ..Default::default()
        };
        let out = l.dps(&warm, &last, None);
        assert_eq!(out["23"], json!(0));
        assert_eq!(out["21"], json!("white"));
    }
}
