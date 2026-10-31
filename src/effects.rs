//! Les effets de Heim : respiration, arc-en-ciel, bougie, lever de soleil, pulsation, alerte.
//!
//! Heim les joue lui-même, pas à pas, avec les commandes que toutes les lampes comprennent :
//! ils marchent donc avec Zigbee, Tuya, Matter ou MQTT. Les lampes en couleur reçoivent
//! luminosité + couleur d'un seul message (`DeviceSet`), les autres la luminosité seule.
//!
//! Le rythme dépend de la connexion : une lampe Zigbee reçoit un pas toutes les 1,5 s avec un
//! fondu de la même durée (c'est la lampe qui glisse, le réseau n'est pas saturé) ; une lampe
//! Tuya un pas toutes les 0,3 s (elle n'a pas de fondu en local). Un effet s'arrête dès que la
//! lampe reçoit une autre commande (interface, scène, automatisme).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::bus::{Bus, Event};
use crate::model::{EntityView, Target};
use crate::registry::RegistryHandle;

/// La liste par défaut : identifiant, nom, couleur nécessaire.
pub const LIST: &[(&str, &str, bool)] = &[
    ("breathe", "Respiration", false),
    ("rainbow", "Arc-en-ciel", true),
    ("candle", "Bougie", true),
    ("sunrise", "Lever de soleil", false),
    ("pulse", "Pulsation", true),
    ("alert", "Alerte", false),
];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub effect: String,
    /// Pulsation : les deux couleurs (`#rrggbb`).
    #[serde(default)]
    pub colors: Vec<String>,
    /// Lever de soleil : durée en minutes (15 par défaut).
    #[serde(default)]
    pub minutes: Option<f64>,
}

/// Un pas d'effet : luminosité (1-255), couleur éventuelle, ou extinction.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub on: bool,
    pub brightness: u8,
    pub color: Option<String>,
}

/// Teinte (degrés), saturation et valeur (0-1) → `#rrggbb`.
pub fn hsv_hex(h: f64, s: f64, v: f64) -> String {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |u: f64| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", to(r), to(g), to(b))
}

fn mix(a: &str, b: &str, t: f64) -> String {
    let p = |s: &str, i: usize| {
        u8::from_str_radix(s.trim_start_matches('#').get(i..i + 2).unwrap_or("ff"), 16)
            .unwrap_or(255) as f64
    };
    let c = |i: usize| {
        (p(a, i) + (p(b, i) - p(a, i)) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", c(0), c(2), c(4))
}

/// Un petit hasard sans dépendance (bougie).
fn noise(n: u64) -> f64 {
    let mut x = n.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xD1B5_4A32_D192_ED03;
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    (x % 10_000) as f64 / 10_000.0
}

/// Le pas `n` d'un effet, `period` secondes après le précédent. `None` : l'effet est fini.
pub fn step(r: &Request, n: u64, period: f64, colour: bool) -> Option<Step> {
    let t = n as f64 * period;
    let s = match r.effect.as_str() {
        "breathe" => {
            let phase = (t / 6.0 * std::f64::consts::TAU).sin() * 0.5 + 0.5;
            Step {
                on: true,
                brightness: (25.0 + phase * 230.0) as u8,
                color: None,
            }
        }
        "rainbow" => Step {
            on: true,
            brightness: 255,
            color: Some(hsv_hex(t / 60.0 * 360.0, 1.0, 1.0)),
        },
        "candle" => {
            let a = noise(n);
            let b = noise(n.wrapping_add(7_777));
            Step {
                on: true,
                brightness: (110.0 + a * 120.0) as u8,
                color: colour.then(|| hsv_hex(18.0 + b * 14.0, 0.85, 1.0)),
            }
        }
        "sunrise" => {
            let total = r.minutes.unwrap_or(15.0).clamp(1.0, 120.0) * 60.0;
            let p = (t / total).min(1.0);
            let colour_now = if p < 0.5 {
                mix("#ff2a00", "#ff8a2a", p * 2.0)
            } else {
                mix("#ff8a2a", "#ffe0b0", (p - 0.5) * 2.0)
            };
            if t > total + period {
                return None;
            }
            Step {
                on: true,
                brightness: (1.0 + p * 254.0) as u8,
                color: colour.then_some(colour_now),
            }
        }
        "pulse" => {
            let a = r
                .colors
                .first()
                .cloned()
                .unwrap_or_else(|| "#ff3355".into());
            let b = r.colors.get(1).cloned().unwrap_or_else(|| "#3355ff".into());
            let which = ((t / 3.0) as u64).is_multiple_of(2);
            Step {
                on: true,
                brightness: 255,
                color: Some(if which { a } else { b }),
            }
        }
        "alert" => {
            // Trois éclairs d'une demi-seconde, puis fin (l'état d'avant est remis).
            let k = (t / 0.5) as u64;
            if k >= 6 {
                return None;
            }
            Step {
                on: k.is_multiple_of(2),
                brightness: 255,
                color: colour.then(|| "#ff0000".to_string()),
            }
        }
        _ => return None,
    };
    Some(s)
}

/// Comment piloter une lampe donnée.
#[derive(Debug, Clone)]
struct Lamp {
    entity_id: String,
    integration: String,
    device_id: Option<String>,
    colour: bool,
    /// Zigbee2MQTT ou Tuya : un message `DeviceSet` complet (couleur, fondu).
    direct: bool,
    before: Target,
}

impl Lamp {
    fn period(&self, effect: &str) -> f64 {
        match (effect, self.integration.as_str()) {
            ("alert", _) => 0.5,
            (_, "tuya") => 0.3,
            (_, "zigbee2mqtt") => 1.5,
            _ => 2.0,
        }
    }
}

/// Lampe → (effet, tâche qui le joue).
type Running = HashMap<String, (String, tokio::task::JoinHandle<()>)>;

#[derive(Clone)]
pub struct Effects {
    running: Arc<Mutex<Running>>,
    bus: Bus,
    registry: RegistryHandle,
    controls: crate::devices::Controls,
}

impl Effects {
    pub fn new(bus: Bus, registry: RegistryHandle, controls: crate::devices::Controls) -> Self {
        let me = Effects {
            running: Default::default(),
            bus,
            registry,
            controls,
        };
        me.spawn_stopper();
        me
    }

    /// Les effets en cours : lampe → effet.
    pub fn running(&self) -> HashMap<String, String> {
        let mut r = self.running.lock().unwrap_or_else(|e| e.into_inner());
        r.retain(|_, (_, h)| !h.is_finished());
        r.iter().map(|(k, (e, _))| (k.clone(), e.clone())).collect()
    }

    pub fn stop(&self, entity_id: &str) {
        if let Some((_, h)) = self
            .running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(entity_id)
        {
            h.abort();
        }
    }

    /// Les effets que cette lampe peut jouer (ceux en couleur demandent une lampe couleur).
    pub fn available(&self, e: &EntityView) -> Vec<Value> {
        let colour = self.has_colour(e);
        LIST.iter()
            .filter(|(_, _, needs)| !needs || colour)
            .map(|(id, name, _)| json!({ "id": id, "name": name }))
            .collect()
    }

    fn has_colour(&self, e: &EntityView) -> bool {
        e.meta.device_id.as_ref().is_some_and(|d| {
            crate::devices::lock(&self.controls)
                .get(d)
                .is_some_and(|c| {
                    c.controls
                        .iter()
                        .any(|x| x.get("kind").and_then(Value::as_str) == Some("color"))
                })
        })
    }

    pub fn start(&self, e: &EntityView, r: Request) -> Result<(), String> {
        if !e.meta.entity_id.starts_with("light.") {
            return Err("les effets sont pour les lampes".into());
        }
        let colour = self.has_colour(e);
        let def = LIST
            .iter()
            .find(|(id, _, _)| *id == r.effect)
            .ok_or("effet inconnu")?;
        if def.2 && !colour {
            return Err("cet effet demande une lampe en couleur".into());
        }
        for c in &r.colors {
            let ok =
                c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit());
            if !ok {
                return Err("couleur invalide (#rrggbb attendu)".into());
            }
        }
        let direct = matches!(e.meta.integration.as_str(), "zigbee2mqtt" | "tuya")
            && e.meta.device_id.is_some();
        let lamp = Lamp {
            entity_id: e.meta.entity_id.clone(),
            integration: e.meta.integration.clone(),
            device_id: e.meta.device_id.clone(),
            colour: colour && direct,
            direct,
            before: e
                .state
                .as_ref()
                .map(Target::from_state)
                .unwrap_or_else(Target::off),
        };
        self.stop(&lamp.entity_id);
        let me = self.clone();
        let effect = r.effect.clone();
        let id = lamp.entity_id.clone();
        let handle = tokio::spawn(async move { me.play(lamp, r).await });
        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, (effect, handle));
        Ok(())
    }

    async fn play(self, lamp: Lamp, r: Request) {
        let period = lamp.period(&r.effect);
        let source = format!("effect:{}", r.effect);
        let mut tick = tokio::time::interval(Duration::from_secs_f64(period));
        let mut n = 0u64;
        loop {
            tick.tick().await;
            let Some(s) = step(&r, n, period, lamp.colour) else {
                break;
            };
            self.send(&lamp, &s, period, &source).await;
            n += 1;
        }
        if r.effect == "alert" {
            self.registry
                .apply(&lamp.entity_id, lamp.before.clone(), &source)
                .await;
        }
        self.running
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&lamp.entity_id);
    }

    async fn send(&self, lamp: &Lamp, s: &Step, period: f64, source: &str) {
        if lamp.direct
            && let Some(device) = &lamp.device_id
        {
            let mut v = Map::new();
            v.insert("state".into(), json!(if s.on { "ON" } else { "OFF" }));
            if s.on {
                let b = if lamp.integration == "zigbee2mqtt" {
                    (s.brightness as f64 * 254.0 / 255.0).round().max(1.0) as u64
                } else {
                    s.brightness.max(1) as u64
                };
                v.insert("brightness".into(), json!(b));
                if let Some(c) = &s.color {
                    v.insert("color".into(), json!({ "hex": c }));
                }
                // Zigbee : la lampe glisse elle-même jusqu'au pas suivant.
                if lamp.integration == "zigbee2mqtt" && period >= 1.0 {
                    v.insert("transition".into(), json!(period));
                }
            }
            self.bus.publish(Event::DeviceSet {
                integration: lamp.integration.clone(),
                device_id: device.clone(),
                values: v,
            });
            return;
        }
        let t = if s.on {
            Target {
                on: true,
                brightness: Some(s.brightness.max(1)),
                color_temp: None,
            }
        } else {
            Target::off()
        };
        self.registry.apply(&lamp.entity_id, t, source).await;
    }

    /// Une autre commande pour la lampe (interface, scène, automatisme) arrête son effet.
    fn spawn_stopper(&self) {
        let me = self.clone();
        let mut rx = self.bus.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(Event::CommandRequested {
                        entity_id, source, ..
                    })
                    | Ok(Event::CommandNoted { entity_id, source })
                        if !source.starts_with("effect:") =>
                    {
                        me.stop(&entity_id);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    _ => {}
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(e: &str) -> Request {
        Request {
            effect: e.into(),
            ..Default::default()
        }
    }

    #[test]
    fn effects_step_and_end() {
        assert_eq!(hsv_hex(0.0, 1.0, 1.0), "#ff0000");
        assert_eq!(hsv_hex(120.0, 1.0, 1.0), "#00ff00");
        let b: Vec<u8> = (0..20)
            .map(|n| step(&req("breathe"), n, 0.3, true).unwrap().brightness)
            .collect();
        assert!(b.iter().min() < b.iter().max());
        assert!(step(&req("alert"), 6, 0.5, true).is_none());
        assert!(!step(&req("alert"), 1, 0.5, true).unwrap().on);
        let sunrise = Request {
            effect: "sunrise".into(),
            minutes: Some(1.0),
            ..Default::default()
        };
        assert_eq!(step(&sunrise, 0, 2.0, true).unwrap().brightness, 1);
        assert!(step(&sunrise, 40, 2.0, true).is_none());
        let pulse = Request {
            effect: "pulse".into(),
            colors: vec!["#111111".into(), "#222222".into()],
            ..Default::default()
        };
        assert_eq!(
            step(&pulse, 0, 1.5, true).unwrap().color.as_deref(),
            Some("#111111")
        );
        assert_eq!(
            step(&pulse, 2, 1.5, true).unwrap().color.as_deref(),
            Some("#222222")
        );
        assert!(step(&req("candle"), 3, 0.3, false).unwrap().color.is_none());
    }
}
