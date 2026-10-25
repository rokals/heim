//! Réglages modifiables depuis l'interface : connexions, lieu (pour le soleil),
//! alertes, tableau de bord... Tout est en base : plus besoin de variables d'environnement.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bus::{Bus, Event};
use crate::store::{StoreHandle, Table};

/// Clés connues, côté noyau. L'interface peut stocker les siennes (préfixe `ui.`).
pub const ZIGBEE2MQTT: &str = "zigbee2mqtt";
pub const LOCATION: &str = "location";
pub const ALERTS: &str = "alerts";
pub const HOME: &str = "home";

/// La maison : son nom, ses étages, ses pièces (dans l'ordre choisi).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeLayout {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub floors: Vec<String>,
    #[serde(default)]
    pub rooms: Vec<RoomDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl HomeLayout {
    pub fn validate(&mut self) -> Result<(), String> {
        self.name = self.name.trim().chars().take(60).collect();
        self.floors.retain(|f| !f.trim().is_empty());
        let mut seen = std::collections::HashSet::new();
        for r in &mut self.rooms {
            r.name = r.name.trim().chars().take(40).collect();
            if r.name.is_empty() {
                return Err("chaque pièce doit avoir un nom".into());
            }
            if !seen.insert(r.name.to_lowercase()) {
                return Err(format!("la pièce « {} » existe deux fois", r.name));
            }
            if r.floor.as_ref().is_some_and(|f| !self.floors.contains(f)) {
                r.floor = None;
            }
        }
        if self.rooms.len() > 80 {
            return Err("80 pièces au plus".into());
        }
        Ok(())
    }
}

/// Taille maximale d'un réglage, pour qu'une interface défaillante ne remplisse pas la carte SD.
pub const MAX_VALUE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Row {
    key: String,
    value: Value,
}

pub struct Settings {
    values: Mutex<BTreeMap<String, Value>>,
    store: StoreHandle,
    bus: Bus,
}

impl Settings {
    pub fn new(rows: Vec<Value>, store: StoreHandle, bus: Bus) -> Arc<Self> {
        let values = rows
            .into_iter()
            .filter_map(|v| serde_json::from_value::<Row>(v).ok())
            .map(|r| (r.key, r.value))
            .collect();
        Arc::new(Self {
            values: Mutex::new(values),
            store,
            bus,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Value>> {
        self.values.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get_raw(&self, key: &str) -> Option<Value> {
        self.lock().get(key).cloned()
    }

    /// Lit un réglage typé ; une valeur illisible est traitée comme absente.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.get_raw(key)
            .and_then(|v| serde_json::from_value(v).ok())
    }

    pub fn get_or_default<T: DeserializeOwned + Default>(&self, key: &str) -> T {
        self.get(key).unwrap_or_default()
    }

    pub fn set_raw(&self, key: &str, value: Value) -> Result<(), String> {
        let size = serde_json::to_vec(&value)
            .map(|v| v.len())
            .unwrap_or(usize::MAX);
        if size > MAX_VALUE_BYTES {
            return Err(format!("réglage trop volumineux ({size} octets)"));
        }
        if key.is_empty()
            || key.len() > 64
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
        {
            return Err("nom de réglage invalide".into());
        }
        self.store.put(
            Table::Settings,
            key,
            &Row {
                key: key.to_string(),
                value: value.clone(),
            },
        );
        self.lock().insert(key.to_string(), value);
        self.bus.publish(Event::SettingsChanged {
            key: key.to_string(),
        });
        Ok(())
    }

    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<(), String> {
        self.set_raw(key, serde_json::to_value(value).map_err(|e| e.to_string())?)
    }
}

/// Connexion à Zigbee2MQTT, telle que saisie dans l'interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zigbee2MqttSettings {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default = "default_topic")]
    pub base_topic: String,
}

fn yes() -> bool {
    true
}

fn default_port() -> u16 {
    1883
}

fn default_topic() -> String {
    "zigbee2mqtt".into()
}

/// Position de la maison, pour calculer lever et coucher du soleil sans Internet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

impl Default for Location {
    /// Centre de la France, en attendant que l'utilisateur précise.
    fn default() -> Self {
        Self {
            latitude: 46.6,
            longitude: 2.4,
        }
    }
}

/// Alertes prêtes à l'emploi, sans rien configurer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertSettings {
    #[serde(default = "yes")]
    pub battery: bool,
    #[serde(default = "default_battery_threshold")]
    pub battery_threshold: u8,
    #[serde(default = "yes")]
    pub silent: bool,
    /// Un appareil muet depuis ce nombre d'heures est signalé.
    #[serde(default = "default_silent_hours")]
    pub silent_hours: u32,
    #[serde(default = "yes")]
    pub leak: bool,
    /// Zigbee2MQTT (ou Tuya, Matter) injoignable depuis 5 minutes.
    #[serde(default = "yes")]
    pub integrations: bool,
}

fn default_battery_threshold() -> u8 {
    15
}

fn default_silent_hours() -> u32 {
    24
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            battery: true,
            battery_threshold: default_battery_threshold(),
            silent: true,
            silent_hours: default_silent_hours(),
            leak: true,
            integrations: true,
        }
    }
}
