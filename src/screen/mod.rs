//! Plugin « Écran 3,5" » : un petit écran tactile SPI branché sur les broches du Pi (type
//! Waveshare 3,5" A, MPI3501, « 3.5inch RPi Display »), piloté par Heim lui-même, sans
//! navigateur ni bureau graphique.
//!
//! - Activer le plugin dépose une demande (`display.request`) que le service root
//!   `heim-system` applique dans `config.txt` (overlay `piscreen`, liste fermée) ; au
//!   redémarrage suivant, l'écran apparaît comme un framebuffer `/dev/fbN`.
//! - Heim dessine alors les tuiles du tableau choisi (3 × 2 par page), l'heure, et lit le
//!   tactile : un appui allume ou éteint. Au premier lancement, trois croix à toucher
//!   calibrent le tactile résistif.
//! - Mise en veille (écran noir) après quelques minutes sans appui ; un appui la réveille.

pub mod gfx;
#[cfg(target_os = "linux")]
pub mod hw;

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const SETTINGS_KEY: &str = "screen_gpio";
/// La rotation réglée dans config.txt : l'écran en paysage 480 × 320.
pub const NATIVE_ROTATE: u16 = 90;
pub const PLUGIN_ID: &str = "screen35";
/// Les modèles acceptés par le service root (même liste dans `heim-system`).
pub const MODELS: &[(&str, &str)] = &[(
    "piscreen",
    "3,5\" générique (Waveshare A, MPI3501, « 3.5inch RPi Display »)",
)];

/// Coordonnées brutes du tactile → pixels de l'écran (en paysage, avant la rotation faite
/// par Heim) : x = ax·brut + bx, après échange éventuel des axes. Faite une fois pour toutes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Calib {
    pub swap: bool,
    pub ax: f64,
    pub bx: f64,
    pub ay: f64,
    pub by: f64,
}

impl Calib {
    pub fn map(&self, rx: i32, ry: i32) -> (i32, i32) {
        let (rx, ry) = if self.swap { (ry, rx) } else { (rx, ry) };
        (
            (self.ax * rx as f64 + self.bx).round() as i32,
            (self.ay * ry as f64 + self.by).round() as i32,
        )
    }

    /// Trois croix touchées : haut-gauche `a`, haut-droite `b`, bas-gauche `c` (positions
    /// écran `pa`, `pb`, `pc`, valeurs brutes `ra`, `rb`, `rc`).
    pub fn from_points(
        pa: (i32, i32),
        pb: (i32, i32),
        pc: (i32, i32),
        ra: (i32, i32),
        rb: (i32, i32),
        rc: (i32, i32),
    ) -> Option<Calib> {
        // Aller de a vers b ne change que x à l'écran : l'axe brut qui bouge le plus est x.
        let swap = (rb.1 - ra.1).abs() > (rb.0 - ra.0).abs();
        let pick = |r: (i32, i32)| if swap { (r.1, r.0) } else { r };
        let (a, b, c) = (pick(ra), pick(rb), pick(rc));
        let dx = (b.0 - a.0) as f64;
        let dy = (c.1 - a.1) as f64;
        if dx.abs() < 50.0 || dy.abs() < 50.0 {
            return None;
        }
        let ax = (pb.0 - pa.0) as f64 / dx;
        let ay = (pc.1 - pa.1) as f64 / dy;
        Some(Calib {
            swap,
            ax,
            bx: pa.0 as f64 - ax * a.0 as f64,
            ay,
            by: pa.1 as f64 - ay * a.1 as f64,
        })
    }
}

/// Rotation faite par Heim : coordonnées logiques (ce qu'on dessine) ↔ coordonnées de
/// l'écran (`nw` × `nh`, paysage).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rot {
    pub deg: u16,
    pub nw: i32,
    pub nh: i32,
}

impl Rot {
    /// Taille logique.
    pub fn size(&self) -> (i32, i32) {
        if self.deg == 90 || self.deg == 270 {
            (self.nh, self.nw)
        } else {
            (self.nw, self.nh)
        }
    }

    pub fn to_native(&self, x: i32, y: i32) -> (i32, i32) {
        match self.deg {
            180 => (self.nw - 1 - x, self.nh - 1 - y),
            90 => (y, self.nh - 1 - x),
            270 => (self.nw - 1 - y, x),
            _ => (x, y),
        }
    }

    pub fn to_logical(&self, nx: i32, ny: i32) -> (i32, i32) {
        match self.deg {
            180 => (self.nw - 1 - nx, self.nh - 1 - ny),
            90 => (self.nh - 1 - ny, nx),
            270 => (ny, self.nw - 1 - nx),
            _ => (nx, ny),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "d_model")]
    pub model: String,
    /// Orientation, faite par Heim (sans redémarrage) : 0 paysage, 180 paysage retourné,
    /// 90 et 270 portrait. L'écran lui-même reste réglé en paysage (overlay `rotate=90`).
    #[serde(default)]
    pub rotate: u16,
    /// Une page « Box » en tête : processeur, mémoire, carte SD, température.
    #[serde(default = "d_true")]
    pub perf_page: bool,
    /// Le tableau affiché (vide : le premier).
    #[serde(default)]
    pub board: Option<String>,
    /// Mise en veille après tant de minutes sans appui (0 : jamais).
    #[serde(default = "d_sleep")]
    pub sleep_min: u32,
    #[serde(default)]
    pub calib: Option<Calib>,
    /// Vrai depuis la 0.19.1 : `rotate` est la rotation faite par Heim (avant : celle de
    /// config.txt, 90 par défaut).
    #[serde(default)]
    pub soft_rotate: bool,
}

fn d_model() -> String {
    "piscreen".into()
}
fn d_true() -> bool {
    true
}
fn d_sleep() -> u32 {
    5
}

impl Default for ScreenSettings {
    fn default() -> Self {
        ScreenSettings {
            enabled: false,
            model: d_model(),
            rotate: 0,
            perf_page: true,
            soft_rotate: true,
            board: None,
            sleep_min: d_sleep(),
            calib: None,
        }
    }
}

/// Demande au service root de régler `config.txt` (modèle et rotation, ou rien).
pub fn request_overlay(data: &Path, s: &ScreenSettings) -> Result<(), String> {
    let model = if s.enabled { s.model.as_str() } else { "none" };
    if model != "none" && !MODELS.iter().any(|(m, _)| *m == model) {
        return Err("modèle d'écran inconnu".into());
    }
    // L'écran reste toujours en paysage : c'est Heim qui tourne l'image.
    std::fs::write(
        data.join("display.request"),
        format!("model={model}\nrotate={}\n", NATIVE_ROTATE),
    )
    .map_err(|e| e.to_string())
}

/// L'état pour Réglages › Écrans.
pub fn status(data: &Path, s: &ScreenSettings) -> Value {
    let result = std::fs::read_to_string(data.join("display.result")).unwrap_or_default();
    let get = |k: &str| {
        result
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")))
            .map(str::to_string)
    };
    #[cfg(target_os = "linux")]
    let (fb, touch) = (
        hw::find_fb().map(|(p, d)| format!("{} ({d})", p.display())),
        hw::find_touch().map(|p| p.display().to_string()),
    );
    #[cfg(not(target_os = "linux"))]
    let (fb, touch): (Option<String>, Option<String>) = (None, None);
    let applied = get("model");
    let wanted = if s.enabled {
        s.model.clone()
    } else {
        "none".into()
    };
    json!({
        "settings": s,
        "models": MODELS.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect::<Vec<_>>(),
        "framebuffer": fb,
        "touch": touch,
        "pending": data.join("display.request").exists(),
        "applied": applied,
        "applied_rotate": get("rotate").and_then(|r| r.parse::<u16>().ok()),
        "result": get("status"),
        "message": get("message"),
        // Réglé dans config.txt mais pas encore actif (ou réglage d'avant à remplacer) : il
        // faut redémarrer.
        "needs_reboot": s.enabled && applied.as_deref() == Some(wanted.as_str())
            && (fb.is_none() || get("rotate").and_then(|r| r.parse::<u16>().ok()) != Some(NATIVE_ROTATE)),
    })
}

/// Les entités du tableau, dans l'ordre (toute clé `entity_id` du tableau choisi).
pub fn board_entities(boards: &Value, wanted: Option<&str>) -> (String, Vec<String>) {
    // Réglage « dashboards » : `{"boards": [...]}` (ou une liste, par sécurité).
    let list = boards
        .get("boards")
        .and_then(Value::as_array)
        .or_else(|| boards.as_array())
        .cloned()
        .unwrap_or_default();
    let board = wanted
        .and_then(|w| {
            list.iter()
                .find(|b| b.get("id").and_then(Value::as_str) == Some(w))
        })
        .or_else(|| list.first())
        .cloned()
        .unwrap_or(Value::Null);
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                if let Some(id) = m.get("entity_id").and_then(Value::as_str)
                    && !out.iter().any(|x| x == id)
                {
                    out.push(id.to_string());
                }
                for (k, x) in m {
                    if k != "entity_id" {
                        walk(x, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(&board, &mut out);
    let name = board
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Accueil")
        .to_string();
    (name, out)
}

/// Le texte d'état d'une tuile.
pub fn state_text(e: &crate::model::EntityView) -> (String, bool) {
    let Some(s) = &e.state else {
        return ("…".into(), false);
    };
    if !s.available {
        return ("Injoignable".into(), false);
    }
    match &s.value {
        Value::String(v) if v == "on" => {
            let b = s.attributes.get("brightness").and_then(Value::as_f64);
            match b {
                Some(b) if e.meta.entity_id.starts_with("light.") => (
                    format!("Allumé · {} %", (b / 255.0 * 100.0).round().max(1.0)),
                    true,
                ),
                _ => ("Allumé".into(), true),
            }
        }
        Value::String(v) if v == "off" => ("Éteint".into(), false),
        Value::Number(n) => {
            let unit = e
                .meta
                .capabilities
                .get("unit")
                .and_then(Value::as_str)
                .unwrap_or("");
            let v = n.as_f64().unwrap_or(0.0);
            let shown = if v.fract() == 0.0 {
                format!("{v:.0}")
            } else {
                format!("{v:.1}")
            };
            (format!("{shown} {unit}").trim().to_string(), false)
        }
        Value::String(v) => (v.clone(), false),
        _ => ("—".into(), false),
    }
}

#[cfg(target_os = "linux")]
mod run;
#[cfg(target_os = "linux")]
pub use run::spawn;

/// Ailleurs que sous Linux, pas d'écran à piloter.
#[cfg(not(target_os = "linux"))]
pub fn spawn(_core: crate::Core) -> Option<tokio::task::JoinHandle<()>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_finds_swapped_and_inverted_axes() {
        // Tactile tourné : x écran suit le y brut, y écran suit x brut à l'envers.
        let raw = |x: i32, y: i32| (4000 - y * 10, 200 + x * 8);
        let (pa, pb, pc) = ((40, 40), (440, 40), (40, 280));
        let c = Calib::from_points(pa, pb, pc, raw(40, 40), raw(440, 40), raw(40, 280)).unwrap();
        assert!(c.swap);
        let (rx, ry) = raw(240, 160);
        assert_eq!(c.map(rx, ry), (240, 160));
    }

    #[test]
    fn rotations_go_back_and_forth() {
        for deg in [0, 90, 180, 270] {
            let r = Rot {
                deg,
                nw: 480,
                nh: 320,
            };
            let (w, h) = r.size();
            for (x, y) in [(0, 0), (w - 1, 0), (10, h - 1), (123, 45)] {
                let (nx, ny) = r.to_native(x, y);
                assert!((0..480).contains(&nx) && (0..320).contains(&ny), "{deg}");
                assert_eq!(r.to_logical(nx, ny), (x, y), "{deg}");
            }
        }
    }

    #[test]
    fn board_entities_in_order() {
        let boards = json!({ "boards": [
            { "id": "a", "name": "Accueil", "sections": [{ "cards": [
                { "entity_id": "light.salon" }, { "type": "title" }, { "entity_id": "switch.prise" }
            ]}]},
            { "id": "b", "name": "Cuisine", "sections": [{ "cards": [{ "entity_id": "light.cuisine" }]}]}
        ]});
        assert_eq!(
            board_entities(&boards, None),
            (
                "Accueil".into(),
                vec!["light.salon".into(), "switch.prise".into()]
            )
        );
        assert_eq!(
            board_entities(&boards, Some("b")).1,
            vec!["light.cuisine".to_string()]
        );
    }
}
