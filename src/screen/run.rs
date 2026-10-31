//! La boucle de l'écran : dessin des tuiles, tactile, calibrage, veille.

use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::mpsc;

use super::gfx::{Canvas, Rgb};
use super::hw::{self, Touch};
use super::{Calib, Rot, SETTINGS_KEY, ScreenSettings, board_entities, state_text};
use crate::Core;
use crate::bus::Event;
use crate::model::Action;

const BG: &str = "#12181d";
const TILE: &str = "#1e272e";
const TILE_ON: &str = "#f2b33d";
const INK: &str = "#e4ebe8";
const INK_ON: &str = "#1c2b33";
const MUTED: &str = "#8ea3ab";
const COLS: i32 = 3;
const ROWS: i32 = 2;
const TOP: i32 = 48;
const GAP: i32 = 8;

enum Mode {
    /// Croix à toucher : étape 0, 1, 2 et valeurs brutes déjà lues.
    Calibrate(usize, Vec<(i32, i32)>),
    Tiles,
    Asleep,
}

/// Lance l'écran s'il est branché et activé. Renvoie la tâche (à arrêter si on le désactive).
pub fn spawn(core: Core) -> Option<tokio::task::JoinHandle<()>> {
    let s: ScreenSettings = core.settings.get(SETTINGS_KEY).unwrap_or_default();
    if !s.enabled {
        return None;
    }
    Some(tokio::spawn(async move {
        // L'écran apparaît au démarrage (overlay) : on le cherche un moment, puis on abandonne.
        let mut found = None;
        for _ in 0..30 {
            if let Some(f) = hw::find_fb() {
                found = Some(f);
                break;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let Some((dev, driver)) = found else {
            tracing::info!(
                "écran : aucun écran SPI trouvé (redémarrage nécessaire après activation ?)"
            );
            return;
        };
        let fb = match hw::Fb::open(&dev) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!("écran : {e}");
                return;
            }
        };
        tracing::info!("écran : {} {}×{} ({driver})", dev.display(), fb.w, fb.h);
        let (ttx, trx) = mpsc::channel(32);
        if let Some(t) = hw::find_touch() {
            hw::spawn_touch(t, ttx);
        } else {
            tracing::info!("écran : pas de tactile trouvé, affichage seul");
        }
        Screen::new(core, fb).run(trx).await;
    }))
}

struct Screen {
    core: Core,
    fb: hw::Fb,
    c: Canvas,
    mode: Mode,
    page: usize,
    last_touch: Instant,
    down: Option<(i32, i32)>,
    /// Zones touchables de la page affichée : (x, y, l, h, entité).
    zones: Vec<(i32, i32, i32, i32, String)>,
    pages: usize,
    rot: Rot,
}

impl Screen {
    fn new(core: Core, fb: hw::Fb) -> Screen {
        let s: ScreenSettings = core.settings.get(SETTINGS_KEY).unwrap_or_default();
        let rot = Rot {
            deg: s.rotate,
            nw: fb.w,
            nh: fb.h,
        };
        let (lw, lh) = rot.size();
        let c = Canvas::new(lw, lh);
        Screen {
            mode: if s.calib.is_some() {
                Mode::Tiles
            } else {
                Mode::Calibrate(0, Vec::new())
            },
            core,
            fb,
            c,
            page: 0,
            last_touch: Instant::now(),
            down: None,
            zones: Vec::new(),
            pages: 1,
            rot,
        }
    }

    fn settings(&self) -> ScreenSettings {
        self.core.settings.get(SETTINGS_KEY).unwrap_or_default()
    }

    /// Les trois croix du calibrage, en pixels.
    /// Les trois croix du calibrage, sur l'écran en paysage (haut-gauche, haut-droite,
    /// bas-gauche) ; on les dessine là où elles tombent après rotation.
    fn native_targets(&self) -> [(i32, i32); 3] {
        let (w, h) = (self.rot.nw, self.rot.nh);
        [(40, 40), (w - 40, 40), (40, h - 40)]
    }

    fn targets(&self) -> [(i32, i32); 3] {
        let rot = self.rot;
        self.native_targets().map(|(x, y)| rot.to_logical(x, y))
    }

    async fn run(mut self, mut touches: mpsc::Receiver<Touch>) {
        let mut bus = self.core.bus.subscribe();
        let mut minute = tokio::time::interval(Duration::from_secs(20));
        let mut dirty = true;
        loop {
            if dirty {
                self.draw().await;
                if let Err(e) = self.fb.show(&self.c.px, self.c.w, self.rot) {
                    tracing::warn!("écran : écriture impossible ({e})");
                    return;
                }
                dirty = false;
            }
            tokio::select! {
                t = touches.recv() => match t {
                    Some(t) => dirty = self.on_touch(t).await,
                    None => {
                        // Plus de tactile : on continue d'afficher.
                        std::future::pending::<()>().await;
                    }
                },
                ev = bus.recv() => match ev {
                    Ok(Event::StateChanged { entity_id, .. }) => {
                        dirty = matches!(self.mode, Mode::Tiles)
                            && self.zones.iter().any(|z| z.4 == entity_id);
                    }
                    Ok(Event::SettingsChanged { .. }) => dirty = !matches!(self.mode, Mode::Asleep),
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    _ => {}
                },
                _ = minute.tick() => {
                    let s = self.settings();
                    if s.enabled && s.calib.is_none() && !matches!(self.mode, Mode::Calibrate(..)) {
                        // Recalibrage demandé depuis Réglages.
                        self.mode = Mode::Calibrate(0, Vec::new());
                        dirty = true;
                    } else if s.sleep_min > 0
                        && matches!(self.mode, Mode::Tiles)
                        && self.last_touch.elapsed() > Duration::from_secs(s.sleep_min as u64 * 60)
                    {
                        self.mode = Mode::Asleep;
                        dirty = true;
                    } else {
                        dirty |= matches!(self.mode, Mode::Tiles); // l'heure
                    }
                }
            }
        }
    }

    /// Un appui : renvoie vrai s'il faut redessiner.
    async fn on_touch(&mut self, t: Touch) -> bool {
        self.last_touch = Instant::now();
        match t {
            Touch::Down(x, y) => {
                self.down = Some((x, y));
                false
            }
            Touch::Up(..) => {
                let Some((rx, ry)) = self.down.take() else {
                    return false;
                };
                let targets = self.native_targets();
                match &mut self.mode {
                    Mode::Asleep => {
                        self.mode = Mode::Tiles;
                        true
                    }
                    Mode::Calibrate(step, raw) => {
                        raw.push((rx, ry));
                        *step += 1;
                        if *step < 3 {
                            return true;
                        }
                        // Le tactile se règle sur l'écran lui-même (paysage) : il reste juste
                        // quand on change l'orientation.
                        let [a, b, c] = targets;
                        let raw = raw.clone();
                        match Calib::from_points(a, b, c, raw[0], raw[1], raw[2]) {
                            Some(cal) => {
                                let mut s = self.settings();
                                s.calib = Some(cal);
                                let _ = self.core.settings.set(SETTINGS_KEY, &s);
                                self.mode = Mode::Tiles;
                            }
                            None => self.mode = Mode::Calibrate(0, Vec::new()),
                        }
                        true
                    }
                    Mode::Tiles => {
                        let Some(cal) = self.settings().calib else {
                            return false;
                        };
                        let (nx, ny) = cal.map(rx, ry);
                        let (x, y) = self.rot.to_logical(nx, ny);
                        self.tap(x, y).await
                    }
                }
            }
        }
    }

    async fn tap(&mut self, x: i32, y: i32) -> bool {
        if y < TOP {
            // Barre du haut : moitié gauche page précédente, moitié droite page suivante.
            if self.pages > 1 {
                self.page = if x < self.c.w / 2 {
                    (self.page + self.pages - 1) % self.pages
                } else {
                    (self.page + 1) % self.pages
                };
                return true;
            }
            return false;
        }
        let hit = self
            .zones
            .iter()
            .find(|z| x >= z.0 && x < z.0 + z.2 && y >= z.1 && y < z.1 + z.3)
            .map(|z| z.4.clone());
        if let Some(id) = hit {
            let action = if id.starts_with("scene.") {
                Some(Action::TurnOn {
                    brightness: None,
                    color_temp: None,
                })
            } else if ["light.", "switch.", "group.", "fan.", "automation."]
                .iter()
                .any(|p| id.starts_with(p))
            {
                Some(Action::Toggle)
            } else {
                None
            };
            if let Some(a) = action {
                let _ = self.core.registry.command(&id, a, "screen:box").await;
            }
        }
        false
    }

    async fn draw(&mut self) {
        let (w, h) = (self.c.w, self.c.h);
        self.c.clear(Rgb::hex(BG));
        match &self.mode {
            Mode::Asleep => {}
            Mode::Calibrate(step, _) => {
                let (x, y) = self.targets()[*step];
                let ink = Rgb::hex(INK);
                self.c.round_rect(x - 18, y - 1, 36, 3, 1, ink);
                self.c.round_rect(x - 1, y - 18, 3, 36, 1, ink);
                self.c.circle(x, y, 4, Rgb::hex(TILE_ON));
                self.c
                    .text_center(0, w, h / 2 - 6, "Touchez le centre de la croix", 20, ink);
                self.c.text_center(
                    0,
                    w,
                    h / 2 + 22,
                    &format!("Réglage du tactile · {}/3", step + 1),
                    15,
                    Rgb::hex(MUTED),
                );
            }
            Mode::Tiles => self.draw_tiles().await,
        }
    }

    async fn draw_tiles(&mut self) {
        let s = self.settings();
        let boards = self
            .core
            .settings
            .get::<Value>(crate::dashboards::KEY)
            .unwrap_or(Value::Null);
        let (board_name, ids) = board_entities(&boards, s.board.as_deref());
        let all = self.core.registry.snapshot().await;
        let shown: Vec<_> = ids
            .iter()
            .filter_map(|id| all.iter().find(|e| &e.meta.entity_id == id))
            .collect();
        let per = (COLS * ROWS) as usize;
        let extra = usize::from(s.perf_page);
        self.pages = shown.len().div_ceil(per).max(1) + extra;
        self.page = self.page.min(self.pages - 1);
        let now = chrono::Local::now().format("%H:%M").to_string();

        if s.perf_page && self.page == 0 {
            let perf: Vec<(String, String, f64, String)> = PERF
                .iter()
                .map(|(id, label, max)| {
                    let v = all
                        .iter()
                        .find(|e| e.meta.entity_id == *id)
                        .and_then(|e| e.state.as_ref())
                        .and_then(|st| st.value.as_f64());
                    let unit = if id.ends_with("temperature") {
                        " °C"
                    } else {
                        " %"
                    };
                    let text = v
                        .map(|v| format!("{v:.0}{unit}"))
                        .unwrap_or_else(|| "—".into());
                    (
                        (*label).to_string(),
                        text,
                        v.unwrap_or(0.0) / max,
                        (*id).to_string(),
                    )
                })
                .collect();
            self.zones = paint_perf(&mut self.c, &now, self.page, self.pages, &perf);
            return;
        }
        let tile_page = self.page - extra;
        let tiles: Vec<(String, String, bool, String)> = shown
            .iter()
            .skip(tile_page * per)
            .take(per)
            .map(|e| {
                let (text, on) = state_text(e);
                (e.meta.name.clone(), text, on, e.meta.entity_id.clone())
            })
            .collect();
        self.zones = paint(
            &mut self.c,
            &now,
            &board_name,
            self.page,
            self.pages,
            &tiles,
            shown.is_empty(),
        );
    }
}

/// La page « Box » : entité, nom, maximum de la jauge.
const PERF: &[(&str, &str, f64)] = &[
    ("sensor.heim_cpu", "Processeur", 100.0),
    ("sensor.heim_memory", "Mémoire", 100.0),
    ("sensor.heim_disk", "Carte SD", 100.0),
    ("sensor.heim_temperature", "Température", 85.0),
];

/// La barre du haut : heure, titre, pages.
fn header(c: &mut Canvas, now: &str, title: &str, page: usize, pages: usize) {
    let w = c.w;
    c.text(14, 32, now, 24, Rgb::hex(INK), 100);
    let title = if pages > 1 {
        format!("{title}  {}/{}", page + 1, pages)
    } else {
        title.to_string()
    };
    c.text_center(100, w - 180, 30, &title, 16, Rgb::hex(MUTED));
    if pages > 1 {
        c.text(w - 60, 31, "‹ ›", 22, Rgb::hex(MUTED), 50);
    }
}

fn grid(c: &Canvas) -> (i32, i32) {
    if c.w < c.h { (2, 3) } else { (COLS, ROWS) }
}

/// Dessine la page « Box » (jauges) et renvoie ses zones (pour se redessiner quand elles
/// changent).
pub fn paint_perf(
    c: &mut Canvas,
    now: &str,
    page: usize,
    pages: usize,
    perf: &[(String, String, f64, String)],
) -> Vec<(i32, i32, i32, i32, String)> {
    header(c, now, "Box Heim", page, pages);
    let (w, h) = (c.w, c.h);
    let (cols, rows) = if w < h { (1, 4) } else { (2, 2) };
    let tw = (w - GAP * (cols + 1)) / cols;
    let th = (h - TOP - GAP * (rows + 1)) / rows;
    let mut zones = Vec::new();
    for (i, (label, text, frac, id)) in perf.iter().enumerate() {
        let x = GAP + (i as i32 % cols) * (tw + GAP);
        let y = TOP + GAP + (i as i32 / cols) * (th + GAP);
        c.round_rect(x, y, tw, th, 16, Rgb::hex(TILE));
        let frac = frac.clamp(0.0, 1.0);
        let colour = if frac > 0.9 {
            "#e0677a"
        } else if frac > 0.75 {
            TILE_ON
        } else {
            "#86b38a"
        };
        c.text(x + 14, y + 28, label, 15, Rgb::hex(MUTED), tw - 28);
        c.text(
            x + 14,
            y + th - 26,
            text,
            if rows > 2 { 22 } else { 30 },
            Rgb::hex(INK),
            tw - 28,
        );
        let bw = tw - 28;
        c.round_rect(x + 14, y + th - 16, bw, 6, 3, Rgb::hex("#33424c"));
        c.round_rect(
            x + 14,
            y + th - 16,
            ((bw as f64 * frac) as i32).max(6),
            6,
            3,
            Rgb::hex(colour),
        );
        zones.push((x, y, tw, th, id.clone()));
    }
    zones
}

/// Dessine la page de tuiles et renvoie les zones touchables.
pub fn paint(
    c: &mut Canvas,
    now: &str,
    board_name: &str,
    page: usize,
    pages: usize,
    tiles: &[(String, String, bool, String)],
    empty: bool,
) -> Vec<(i32, i32, i32, i32, String)> {
    let (w, h) = (c.w, c.h);
    header(c, now, board_name, page, pages);
    let (cols, rows) = grid(c);
    let tw = (w - GAP * (cols + 1)) / cols;
    let th = (h - TOP - GAP * (rows + 1)) / rows;
    let mut zones = Vec::new();
    if empty {
        c.text_center(
            0,
            w,
            h / 2,
            "Aucune tuile dans ce tableau",
            18,
            Rgb::hex(MUTED),
        );
        return zones;
    }
    for (i, (name, text, on, id)) in tiles.iter().enumerate() {
        let col = i as i32 % cols;
        let row = i as i32 / cols;
        let x = GAP + col * (tw + GAP);
        let y = TOP + GAP + row * (th + GAP);
        let (bg, ink, sub) = if *on {
            (Rgb::hex(TILE_ON), Rgb::hex(INK_ON), Rgb::hex(INK_ON))
        } else {
            (Rgb::hex(TILE), Rgb::hex(INK), Rgb::hex(MUTED))
        };
        c.round_rect(x, y, tw, th, 16, bg);
        // Pastille d'état en haut à gauche.
        c.circle(
            x + 22,
            y + 22,
            9,
            if *on {
                Rgb::hex(INK_ON)
            } else {
                Rgb::hex("#33424c")
            },
        );
        let (l1, l2) = c.wrap2(name, 17, tw - 20);
        c.text(x + 12, y + th - 46, &l1, 17, ink, tw - 20);
        if let Some(l2) = l2 {
            c.text(x + 12, y + th - 46 + 20, &l2, 17, ink, tw - 20);
            c.text(x + 12, y + th - 6, text, 13, sub, tw - 20);
        } else {
            c.text(x + 12, y + th - 16, text, 14, sub, tw - 20);
        }
        zones.push((x, y, tw, th, id.clone()));
    }
    zones
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `HEIM_SCREEN_PREVIEW=chemin.ppm cargo test screen_preview` : l'image de l'écran.
    #[test]
    fn screen_preview() {
        let mut c = Canvas::new(480, 320);
        c.clear(Rgb::hex(BG));
        let t =
            |n: &str, s: &str, on: bool| (n.to_string(), s.to_string(), on, format!("light.{n}"));
        let tiles = vec![
            t("Salon", "Allumé · 80 %", true),
            t("Eurodomest Smart Wall Washer", "Éteint", false),
            t("Prise café", "Allumé", true),
            t("Température bureau", "21.5 °C", false),
            t("Bandeau LED bibliothèque", "Injoignable", false),
            t("Chambre", "Éteint", false),
        ];
        let zones = paint(&mut c, "15:42", "Accueil", 1, 3, &tiles, false);
        assert_eq!(zones.len(), 6);
        if std::env::var("HEIM_SCREEN_PERF").is_ok() {
            c.clear(Rgb::hex(BG));
            let p = |l: &str, t: &str, f: f64| (l.to_string(), t.to_string(), f, String::new());
            paint_perf(
                &mut c,
                "15:42",
                0,
                3,
                &[
                    p("Processeur", "12 %", 0.12),
                    p("Mémoire", "64 %", 0.64),
                    p("Carte SD", "81 %", 0.81),
                    p("Température", "52 °C", 0.61),
                ],
            );
        }
        if let Ok(path) = std::env::var("HEIM_SCREEN_PREVIEW") {
            let mut out = format!("P6 {} {} 255\n", c.w, c.h).into_bytes();
            for p in &c.px {
                let r = ((p >> 11) & 0x1F) as u8;
                let g = ((p >> 5) & 0x3F) as u8;
                let b = (p & 0x1F) as u8;
                out.extend_from_slice(&[r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2]);
            }
            std::fs::write(path, out).unwrap();
        }
    }
}
