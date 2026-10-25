//! Réglages complets des appareils, quelle que soit l'intégration.
//!
//! Chaque intégration décrit ce qu'un appareil sait régler (« contrôles ») et tient à jour
//! les valeurs reçues. L'interface affiche le tout de la même façon : interrupteurs, listes,
//! curseurs, couleur. Un changement repart vers l'intégration par le bus (`DeviceSet`).
//!
//! Un contrôle est un objet JSON :
//! `{"key": "power_on_behavior", "name": "power_on_behavior", "kind": "enum",
//!   "label": "Power-on behavior", "values": ["off","on","toggle","previous"],
//!   "category": "config", "group": null, "endpoint": null}`
//! - `kind` : `binary` (value_on / value_off), `enum` (values), `numeric` (min, max, step,
//!   unit, presets), `text`, `color` (couleur d'une lampe : format xy ou hs) ;
//! - `key` : chemin de la valeur (« level_config.on_level » pour un réglage imbriqué) ;
//! - `group` : `light`, `switch`... quand le réglage appartient à la commande principale.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Default, Serialize)]
pub struct DeviceControls {
    pub integration: String,
    pub device_id: String,
    pub name: String,
    pub controls: Vec<Value>,
    pub values: Map<String, Value>,
    /// Modèle, fabricant, version du logiciel interne, mise à jour disponible...
    pub info: Map<String, Value>,
}

pub type Controls = Arc<Mutex<HashMap<String, DeviceControls>>>;

pub fn lock(c: &Controls) -> std::sync::MutexGuard<'_, HashMap<String, DeviceControls>> {
    c.lock().unwrap_or_else(|e| e.into_inner())
}

/// Valeur d'un contrôle dans l'état reçu (`a.b` : imbriqué).
pub fn value_at<'a>(values: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    let mut parts = key.split('.');
    let mut cur = values.get(parts.next()?)?;
    for p in parts {
        cur = cur.get(p)?;
    }
    Some(cur)
}

/// `{"level_config.on_level": 100, "color": {...}}` -> `{"level_config": {"on_level": 100}, "color": {...}}`
pub fn nest(flat: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (k, v) in flat {
        let parts: Vec<&str> = k.split('.').collect();
        let mut cur = &mut out;
        for p in &parts[..parts.len() - 1] {
            let entry = cur.entry(p.to_string()).or_insert_with(|| json!({}));
            if !entry.is_object() {
                *entry = json!({});
            }
            cur = entry.as_object_mut().expect("objet");
        }
        cur.insert(parts[parts.len() - 1].to_string(), v.clone());
    }
    out
}

/// Comportement après une coupure de courant : « previous », « on » ou « off ».
/// Renvoie la valeur à envoyer à ce contrôle, s'il s'agit bien de ce réglage-là.
pub fn power_on_value(control: &Value, behavior: &str) -> Option<Value> {
    let name = control.get("name").and_then(Value::as_str)?;
    let kind = control.get("kind").and_then(Value::as_str)?;
    let values: Vec<&str> = control
        .get("values")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let pick = |cands: &[&str]| cands.iter().find(|c| values.contains(c)).map(|c| json!(c));
    match (name, kind) {
        // Zigbee2MQTT (la plupart des ampoules et prises).
        ("power_on_behavior" | "power_on_state" | "restore_mode", "enum") => match behavior {
            "previous" => pick(&["previous", "last", "memory", "restore"]),
            "on" => pick(&["on", "ON", "power_on"]),
            "off" => pick(&["off", "OFF", "power_off"]),
            _ => None,
        },
        // Prises Tuya en Zigbee : « mémoire » oui/non.
        ("power_outage_memory", "binary") => match behavior {
            "previous" => control.get("value_on").cloned(),
            "off" => control.get("value_off").cloned(),
            _ => None,
        },
        ("power_outage_memory", "enum") => match behavior {
            "previous" => pick(&["restore", "previous"]),
            "on" => pick(&["on"]),
            "off" => pick(&["off"]),
            _ => None,
        },
        // Tuya Wi-Fi (code « relay_status »).
        ("relay_status", "enum") => match behavior {
            "previous" => pick(&["last", "memory", "2"]),
            "on" => pick(&["power_on", "on", "1"]),
            "off" => pick(&["power_off", "off", "0"]),
            _ => None,
        },
        _ => None,
    }
}

/// Tous les réglages « après coupure » d'un appareil, prêts à envoyer.
pub fn power_on_payload(d: &DeviceControls, behavior: &str) -> Map<String, Value> {
    d.controls
        .iter()
        .filter_map(|c| {
            let key = c.get("key").and_then(Value::as_str)?;
            Some((key.to_string(), power_on_value(c, behavior)?))
        })
        .collect()
}

/// L'appareil sait-il se régler après une coupure ? (et ses choix possibles)
pub fn power_on_supported(d: &DeviceControls) -> Vec<&'static str> {
    ["previous", "on", "off"]
        .into_iter()
        .filter(|b| !power_on_payload(d, b).is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_and_reads_paths() {
        let mut flat = Map::new();
        flat.insert("level_config.on_level".into(), json!(100));
        flat.insert("state".into(), json!("ON"));
        let n = nest(&flat);
        assert_eq!(n["level_config"]["on_level"], json!(100));
        assert_eq!(value_at(&n, "level_config.on_level"), Some(&json!(100)));
    }

    #[test]
    fn power_on_values_for_each_family() {
        let z2m = json!({"key": "power_on_behavior_l1", "name": "power_on_behavior", "kind": "enum", "values": ["off", "on", "toggle", "previous"]});
        assert_eq!(power_on_value(&z2m, "previous"), Some(json!("previous")));
        let mem = json!({"key": "power_outage_memory", "name": "power_outage_memory", "kind": "binary", "value_on": true, "value_off": false});
        assert_eq!(power_on_value(&mem, "previous"), Some(json!(true)));
        assert_eq!(power_on_value(&mem, "on"), None);
        let tuya = json!({"key": "38", "name": "relay_status", "kind": "enum", "values": ["power_off", "power_on", "last"]});
        assert_eq!(power_on_value(&tuya, "off"), Some(json!("power_off")));
        let other = json!({"key": "child_lock", "name": "child_lock", "kind": "binary"});
        assert_eq!(power_on_value(&other, "on"), None);
    }
}
