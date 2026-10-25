//! Modèle de données : entités, états, cibles et actions.
//!
//! Tout ce que Heim pilote ou mesure est une *entité* (`light.salon`, `sensor.salon_temperature`).
//! Une entité garde deux choses distinctes :
//! - l'**état réel**, rapporté par l'appareil ;
//! - l'**état demandé** (`desired`), ce que le noyau a commandé et attend.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub type EntityId = String;
pub type Attributes = Map<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Light,
    Switch,
    Sensor,
    BinarySensor,
    Scene,
    Timer,
    /// Sirène : se commande, mais beaucoup ne renvoient pas leur état.
    Siren,
    /// Une personne : « home » ou « away » (présence par le Wi-Fi).
    Person,
    /// L'état d'un automatisme : « on » (vrai) ou « off » (faux), comme un drapeau qu'il lève
    /// en se déclenchant et qu'on réarme (le courrier est arrivé, puis relevé).
    Automation,
    /// Un groupe de lumières ou de prises commandé comme un seul appareil.
    Group,
}

impl Domain {
    pub fn prefix(self) -> &'static str {
        match self {
            Domain::Light => "light",
            Domain::Switch => "switch",
            Domain::Sensor => "sensor",
            Domain::BinarySensor => "binary_sensor",
            Domain::Scene => "scene",
            Domain::Timer => "timer",
            Domain::Siren => "siren",
            Domain::Person => "person",
            Domain::Automation => "automation",
            Domain::Group => "group",
        }
    }

    /// Peut recevoir une commande (allumer, éteindre, basculer).
    pub fn controllable(self) -> bool {
        matches!(
            self,
            Domain::Light
                | Domain::Switch
                | Domain::Scene
                | Domain::Timer
                | Domain::Siren
                | Domain::Automation
                | Domain::Group
        )
    }

    /// Appareil physique dont on attend une confirmation après commande.
    pub fn physical(self) -> bool {
        matches!(self, Domain::Light | Domain::Switch)
    }
}

/// Rôle d'une entité dans son appareil : la fonction principale, ou une info technique
/// (batterie, tension, qualité du signal) que l'interface range à part.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    #[default]
    Primary,
    Diagnostic,
}

impl Category {
    fn is_primary(&self) -> bool {
        *self == Category::Primary
    }
}

/// Position d'une entité sur le plan de la maison (coordonnées relatives 0..1).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlanPos {
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub floor: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityMeta {
    pub entity_id: EntityId,
    pub domain: Domain,
    pub name: String,
    /// Intégration propriétaire : "zigbee2mqtt", "virtual", "scenes", "timers"...
    pub integration: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanPos>,
    /// Ce que sait faire l'entité (luminosité, unité, modèle...).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub capabilities: Attributes,
    /// L'utilisateur a renommé ou placé l'entité : une intégration ne doit plus écraser ces choix.
    #[serde(default)]
    pub user_edited: bool,
    #[serde(default, skip_serializing_if = "Category::is_primary")]
    pub category: Category,
    /// Masquée par l'utilisateur : n'apparaît plus dans la maison.
    #[serde(default)]
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// "on" / "off" pour les lumières et prises, un nombre pour un capteur, "running" pour un timer...
    pub value: Value,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub attributes: Attributes,
    pub available: bool,
    /// Dernier changement de `value`.
    pub last_changed: DateTime<Utc>,
    /// Dernier changement significatif (valeur, attribut ou disponibilité).
    pub last_updated: DateTime<Utc>,
    /// Dernier message reçu de l'appareil, même sans changement : « vu il y a 3 min ».
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<DateTime<Utc>>,
}

impl State {
    pub fn new(value: Value, attributes: Attributes, available: bool) -> Self {
        let now = Utc::now();
        Self {
            value,
            attributes,
            available,
            last_changed: now,
            last_updated: now,
            last_seen: None,
        }
    }

    pub fn is_on(&self) -> bool {
        matches!(self.value.as_str(), Some("on" | "running"))
    }

    pub fn brightness(&self) -> Option<u8> {
        self.attributes
            .get("brightness")
            .and_then(Value::as_u64)
            .map(|b| b.min(255) as u8)
    }

    pub fn color_temp(&self) -> Option<u16> {
        self.attributes
            .get("color_temp")
            .and_then(Value::as_u64)
            .map(|c| c.min(u16::MAX as u64) as u16)
    }
}

/// État visé pour une lumière ou une prise. Sert aux commandes et aux scènes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub on: bool,
    /// 0..=255
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    /// En mireds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temp: Option<u16>,
}

impl Target {
    /// Tolérance de luminosité : environ 5 % de 255.
    pub const BRIGHTNESS_TOLERANCE: u8 = 13;
    /// Tolérance de température de couleur, en mireds.
    pub const COLOR_TEMP_TOLERANCE: u16 = 15;

    pub fn on() -> Self {
        Self {
            on: true,
            brightness: None,
            color_temp: None,
        }
    }

    pub fn off() -> Self {
        Self {
            on: false,
            brightness: None,
            color_temp: None,
        }
    }

    /// L'état réel correspond-il à la cible ? Un appareil qui ne rapporte pas
    /// sa luminosité n'est pas jugé sur ce critère.
    pub fn matches(&self, state: &State) -> bool {
        if !state.available || self.on != state.is_on() {
            return false;
        }
        if !self.on {
            return true;
        }
        if let (Some(want), Some(have)) = (self.brightness, state.brightness())
            && want.abs_diff(have) > Self::BRIGHTNESS_TOLERANCE
        {
            return false;
        }
        if let (Some(want), Some(have)) = (self.color_temp, state.color_temp())
            && want.abs_diff(have) > Self::COLOR_TEMP_TOLERANCE
        {
            return false;
        }
        true
    }

    /// Photographie de l'état actuel, pour le restaurer plus tard.
    pub fn from_state(state: &State) -> Self {
        let on = state.is_on();
        Self {
            on,
            brightness: if on { state.brightness() } else { None },
            color_temp: if on { state.color_temp() } else { None },
        }
    }
}

/// Commande envoyée par l'interface, une automatisation ou l'IA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    TurnOn {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        brightness: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        color_temp: Option<u16>,
    },
    TurnOff,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Desired {
    pub target: Target,
    pub since: DateTime<Utc>,
}

/// Vue complète d'une entité, telle qu'exposée par l'API.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntityView {
    #[serde(flatten)]
    pub meta: EntityMeta,
    pub state: Option<State>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired: Option<Desired>,
}

/// "Lampe Salon / Plafond" -> "lampe_salon_plafond"
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_underscore = true;
    for c in input.chars() {
        let c = fold_accent(c);
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_underscore = false;
        } else if !last_underscore {
            out.push('_');
            last_underscore = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

fn fold_accent(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' | 'á' | 'À' | 'Â' | 'Ä' => 'a',
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' => 'e',
        'î' | 'ï' | 'Î' | 'Ï' => 'i',
        'ô' | 'ö' | 'ó' | 'Ô' | 'Ö' => 'o',
        'ù' | 'û' | 'ü' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ø' | 'Ø' => 'o',
        'å' | 'Å' => 'a',
        'æ' | 'Æ' => 'a',
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn light(on: bool, brightness: Option<u64>) -> State {
        let mut attrs = Attributes::new();
        if let Some(b) = brightness {
            attrs.insert("brightness".into(), json!(b));
        }
        State::new(json!(if on { "on" } else { "off" }), attrs, true)
    }

    #[test]
    fn slug() {
        assert_eq!(slugify("Lampe Salon / Plafond"), "lampe_salon_plafond");
        assert_eq!(slugify("  Prise Ça-Marche!  "), "prise_ca_marche");
    }

    #[test]
    fn target_matching_with_tolerance() {
        let t = Target {
            on: true,
            brightness: Some(200),
            color_temp: None,
        };
        assert!(t.matches(&light(true, Some(205))));
        assert!(!t.matches(&light(true, Some(150))));
        assert!(!t.matches(&light(false, Some(200))));
        // Appareil sans luminosité : seul on/off compte.
        assert!(t.matches(&light(true, None)));
        assert!(Target::off().matches(&light(false, Some(10))));
    }

    #[test]
    fn unavailable_never_matches() {
        let mut s = light(true, Some(200));
        s.available = false;
        assert!(!Target::on().matches(&s));
    }

    #[test]
    fn action_json() {
        let a: Action = serde_json::from_str(r#"{"action":"turn_on"}"#).unwrap();
        assert_eq!(
            a,
            Action::TurnOn {
                brightness: None,
                color_temp: None
            }
        );
        let a: Action = serde_json::from_str(r#"{"action":"turn_on","brightness":120}"#).unwrap();
        assert_eq!(
            a,
            Action::TurnOn {
                brightness: Some(120),
                color_temp: None
            }
        );
        let a: Action = serde_json::from_str(r#"{"action":"toggle"}"#).unwrap();
        assert_eq!(a, Action::Toggle);
    }
}
