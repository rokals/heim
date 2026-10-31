//! Découverte MQTT, au format de Home Assistant.
//!
//! Tasmota, ESPHome, Shelly, OpenMQTTGateway… et les plugins externes de Heim annoncent leurs
//! appareils sur `homeassistant/<type>/[<nœud>/]<objet>/config`. Heim lit ces annonces, crée
//! les entités (lumières, prises, capteurs, détecteurs), suit leurs états et envoie les
//! commandes sur les sujets indiqués. Seule une partie du format est prise en charge : ce qui
//! sert aux appareils de la maison (on/off, luminosité, mesures, détecteurs, disponibilité).
//!
//! Les annonces de Zigbee2MQTT sont ignorées : Heim lit déjà Zigbee2MQTT directement.

use std::collections::HashMap;
use std::time::Duration;

use rumqttc::{AsyncClient, Event as MqttEvent, MqttOptions, Packet, QoS};
use serde_json::{Map, Value, json};
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event, IntegrationState};
use crate::model::{Attributes, Category, Domain, EntityMeta, Target, slugify};
use crate::plugins::MqttEnv;
use crate::registry::RegistryHandle;

pub const INTEGRATION: &str = "mqtt_discovery";
pub const PREFIX: &str = "homeassistant";

/// Les abréviations du format (Tasmota s'en sert beaucoup).
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("avty", "availability"),
    ("avty_mode", "availability_mode"),
    ("avty_t", "availability_topic"),
    ("avty_tpl", "availability_template"),
    ("bri_cmd_t", "brightness_command_topic"),
    ("bri_scl", "brightness_scale"),
    ("bri_stat_t", "brightness_state_topic"),
    ("bri_val_tpl", "brightness_value_template"),
    ("cmd_t", "command_topic"),
    ("cmd_tpl", "command_template"),
    ("dev", "device"),
    ("dev_cla", "device_class"),
    ("ent_cat", "entity_category"),
    ("json_attr_t", "json_attributes_topic"),
    ("name", "name"),
    ("o", "origin"),
    ("obj_id", "object_id"),
    ("pl_avail", "payload_available"),
    ("pl_not_avail", "payload_not_available"),
    ("pl_off", "payload_off"),
    ("pl_on", "payload_on"),
    ("ret", "retain"),
    ("schema", "schema"),
    ("stat_cla", "state_class"),
    ("stat_off", "state_off"),
    ("stat_on", "state_on"),
    ("stat_t", "state_topic"),
    ("stat_val_tpl", "state_value_template"),
    ("t", "topic"),
    ("uniq_id", "unique_id"),
    ("unit_of_meas", "unit_of_measurement"),
    ("val_tpl", "value_template"),
    ("bri", "brightness"),
];

const DEVICE_ABBREVIATIONS: &[(&str, &str)] = &[
    ("ids", "identifiers"),
    ("cns", "connections"),
    ("mf", "manufacturer"),
    ("mdl", "model"),
    ("sw", "sw_version"),
    ("hw", "hw_version"),
    ("sa", "suggested_area"),
    ("name", "name"),
];

/// Remet les noms longs, et remplace `~` (le sujet de base) dans les sujets.
pub fn expand(raw: &Map<String, Value>) -> Map<String, Value> {
    let long = |k: &str, table: &[(&str, &str)]| {
        table
            .iter()
            .find(|(a, _)| *a == k)
            .map(|(_, l)| l.to_string())
            .unwrap_or_else(|| k.to_string())
    };
    let base = raw.get("~").and_then(Value::as_str).map(str::to_string);
    let fix = |v: &Value| match (v, &base) {
        (Value::String(s), Some(b)) => {
            if let Some(rest) = s.strip_prefix('~') {
                Value::String(format!("{b}{rest}"))
            } else if let Some(head) = s.strip_suffix('~') {
                Value::String(format!("{head}{b}"))
            } else {
                v.clone()
            }
        }
        _ => v.clone(),
    };
    let mut out = Map::new();
    for (k, v) in raw {
        if k == "~" {
            continue;
        }
        let key = long(k, ABBREVIATIONS);
        let v = match (key.as_str(), v) {
            ("device", Value::Object(d)) => Value::Object(
                d.iter()
                    .map(|(k, v)| (long(k, DEVICE_ABBREVIATIONS), v.clone()))
                    .collect(),
            ),
            ("availability", Value::Array(list)) => Value::Array(
                list.iter()
                    .map(|a| match a {
                        Value::Object(o) => Value::Object(
                            o.iter()
                                .map(|(k, v)| {
                                    let k = long(k, ABBREVIATIONS);
                                    let v = if k == "topic" { fix(v) } else { v.clone() };
                                    (k, v)
                                })
                                .collect(),
                        ),
                        other => other.clone(),
                    })
                    .collect(),
            ),
            (k, v) if k.ends_with("_topic") => fix(v),
            _ => v.clone(),
        };
        out.insert(key, v);
    }
    out
}

/// Un petit bout des gabarits Jinja de Home Assistant : `{{ value }}`, `{{ value_json.a.b }}`,
/// `{{ value_json['A']['B'] }}`, avec `| float`, `| int`, `| round(n)`. Le reste : `None`.
pub fn render(template: &str, payload: &str) -> Option<Value> {
    let t = template.trim();
    let inner = t.strip_prefix("{{")?.strip_suffix("}}")?.trim();
    let mut parts = inner.split('|').map(str::trim);
    let expr = parts.next()?;
    let mut v: Value = if expr == "value" {
        Value::String(payload.to_string())
    } else {
        let path = expr.strip_prefix("value_json")?;
        let mut cur: Value = serde_json::from_str(payload).ok()?;
        let mut rest = path.trim();
        while !rest.is_empty() {
            if let Some(r) = rest.strip_prefix('.') {
                let end = r.find(['.', '[']).unwrap_or(r.len());
                cur = cur.get(&r[..end])?.clone();
                rest = &r[end..];
            } else {
                let r = rest.strip_prefix('[')?;
                let end = r.find(']')?;
                let key = r[..end].trim().trim_matches(['\'', '"']);
                cur = match key.parse::<usize>() {
                    Ok(i) if cur.is_array() => cur.get(i)?.clone(),
                    _ => cur.get(key)?.clone(),
                };
                rest = &r[end + 1..];
            }
        }
        cur
    };
    for f in parts {
        let num = || match &v {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse::<f64>().ok(),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        };
        v = if f.starts_with("float") {
            json!(num()?)
        } else if f.starts_with("int") {
            json!(num()?.trunc() as i64)
        } else if let Some(r) = f.strip_prefix("round") {
            let n: i32 = r
                .trim()
                .trim_start_matches('(')
                .trim_end_matches(')')
                .trim()
                .parse()
                .unwrap_or(0);
            let p = 10f64.powi(n);
            json!((num()? * p).round() / p)
        } else if f == "lower" {
            json!(v.as_str()?.to_lowercase())
        } else if f == "upper" {
            json!(v.as_str()?.to_uppercase())
        } else {
            return None;
        };
    }
    Some(v)
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Light {
        json: bool,
        payload_on: String,
        payload_off: String,
        brightness: Option<(Option<String>, u64)>,
    },
    Switch {
        payload_on: String,
        payload_off: String,
        state_on: String,
        state_off: String,
    },
    Sensor,
    Binary {
        payload_on: String,
        payload_off: String,
    },
}

#[derive(Debug, Clone)]
pub struct Item {
    pub entity_id: String,
    pub kind: Kind,
    pub state_topic: Option<String>,
    pub command_topic: Option<String>,
    pub value_template: Option<String>,
    pub brightness_state_topic: Option<String>,
    pub brightness_value_template: Option<String>,
    pub availability: Vec<(String, String, String)>,
    pub meta: EntityMeta,
}

/// Une annonce → une entité. `None` : type non pris en charge, ou annonce de Zigbee2MQTT.
pub fn parse_config(component: &str, object: &str, raw: &Map<String, Value>) -> Option<Item> {
    let c = expand(raw);
    let s = |k: &str| c.get(k).map(text).filter(|v| !v.is_empty());
    let origin = c
        .get("origin")
        .and_then(|o| o.get("name"))
        .map(text)
        .unwrap_or_default();
    if origin.eq_ignore_ascii_case("zigbee2mqtt")
        || s("unique_id").is_some_and(|u| u.ends_with("_zigbee2mqtt"))
    {
        return None;
    }
    let (domain, kind) = match component {
        "light" => {
            let json = s("schema").as_deref() == Some("json");
            let brightness = if json {
                (c.get("brightness").and_then(Value::as_bool) == Some(true)).then(|| {
                    (
                        None,
                        c.get("brightness_scale")
                            .and_then(Value::as_u64)
                            .unwrap_or(255),
                    )
                })
            } else {
                s("brightness_command_topic").map(|t| {
                    (
                        Some(t),
                        c.get("brightness_scale")
                            .and_then(Value::as_u64)
                            .unwrap_or(255),
                    )
                })
            };
            (
                Domain::Light,
                Kind::Light {
                    json,
                    payload_on: s("payload_on").unwrap_or_else(|| "ON".into()),
                    payload_off: s("payload_off").unwrap_or_else(|| "OFF".into()),
                    brightness,
                },
            )
        }
        "switch" => {
            let on = s("payload_on").unwrap_or_else(|| "ON".into());
            let off = s("payload_off").unwrap_or_else(|| "OFF".into());
            (
                Domain::Switch,
                Kind::Switch {
                    state_on: s("state_on").unwrap_or_else(|| on.clone()),
                    state_off: s("state_off").unwrap_or_else(|| off.clone()),
                    payload_on: on,
                    payload_off: off,
                },
            )
        }
        "sensor" => (Domain::Sensor, Kind::Sensor),
        "binary_sensor" => (
            Domain::BinarySensor,
            Kind::Binary {
                payload_on: s("payload_on").unwrap_or_else(|| "ON".into()),
                payload_off: s("payload_off").unwrap_or_else(|| "OFF".into()),
            },
        ),
        _ => return None,
    };
    let unique = s("unique_id").unwrap_or_else(|| format!("{component}_{object}"));
    let entity_id = format!("{}.mqtt_{}", domain.prefix(), slugify(&unique));
    let device = c.get("device").cloned().unwrap_or(Value::Null);
    let device_name = device.get("name").map(text).filter(|n| !n.is_empty());
    let device_id = device
        .get("identifiers")
        .and_then(|i| match i {
            Value::Array(a) => a.first().map(text),
            other => Some(text(other)),
        })
        .filter(|i| !i.is_empty())
        .map(|i| format!("mqtt:{}", slugify(&i)));
    let entity_name = s("name");
    let name = match (&device_name, &entity_name) {
        (Some(d), Some(e)) if !e.starts_with(d.as_str()) => format!("{d} {e}"),
        (_, Some(e)) => e.clone(),
        (Some(d), None) => d.clone(),
        (None, None) => object.replace('_', " "),
    };
    let mut caps = Attributes::new();
    if let Some(d) = &device_name {
        caps.insert("device_name".into(), json!(d));
    }
    for (k, from) in [
        ("model", "model"),
        ("manufacturer", "manufacturer"),
        ("sw_version", "sw_version"),
    ] {
        if let Some(v) = device.get(from).map(text).filter(|v| !v.is_empty()) {
            caps.insert(k.into(), json!(v));
        }
    }
    if let Some(dc) = s("device_class") {
        caps.insert("kind".into(), json!(dc));
    }
    if let Some(u) = s("unit_of_measurement") {
        caps.insert("unit".into(), json!(u));
    }
    if let Kind::Light {
        brightness: Some(_),
        ..
    } = &kind
    {
        caps.insert("brightness".into(), json!(true));
    }
    let category = match s("entity_category").as_deref() {
        Some("diagnostic") | Some("config") => Category::Diagnostic,
        _ => Category::Primary,
    };
    let mut availability = Vec::new();
    let on = s("payload_available").unwrap_or_else(|| "online".into());
    let off = s("payload_not_available").unwrap_or_else(|| "offline".into());
    if let Some(t) = s("availability_topic") {
        availability.push((t, on.clone(), off.clone()));
    }
    if let Some(Value::Array(list)) = c.get("availability") {
        for a in list {
            if let Some(t) = a.get("topic").map(text) {
                let on = a
                    .get("payload_available")
                    .map(text)
                    .unwrap_or_else(|| on.clone());
                let off = a
                    .get("payload_not_available")
                    .map(text)
                    .unwrap_or_else(|| off.clone());
                availability.push((t, on, off));
            }
        }
    }
    Some(Item {
        meta: EntityMeta {
            entity_id: entity_id.clone(),
            domain,
            name,
            integration: INTEGRATION.into(),
            device_id,
            room: device
                .get("suggested_area")
                .map(text)
                .filter(|a| !a.is_empty()),
            plan: None,
            capabilities: caps,
            user_edited: false,
            category,
            hidden: false,
        },
        entity_id,
        kind,
        state_topic: s("state_topic"),
        command_topic: s("command_topic"),
        value_template: s("value_template").or_else(|| s("state_value_template")),
        brightness_state_topic: s("brightness_state_topic"),
        brightness_value_template: s("brightness_value_template"),
        availability,
    })
}

/// Un message reçu sur le sujet d'état → (valeur, attributs).
pub fn state_of(item: &Item, payload: &str) -> Option<(Value, Attributes)> {
    let value = match &item.value_template {
        Some(t) => render(t, payload).unwrap_or_else(|| Value::String(payload.to_string())),
        None => Value::String(payload.to_string()),
    };
    let mut attrs = Attributes::new();
    match &item.kind {
        Kind::Light {
            json: true,
            brightness,
            ..
        } => {
            let p: Value = serde_json::from_str(payload).ok()?;
            let on = p.get("state").map(text)?.eq_ignore_ascii_case("on");
            if let (Some((_, scale)), Some(b)) =
                (brightness, p.get("brightness").and_then(Value::as_u64))
            {
                attrs.insert("brightness".into(), json!(scaled(b, *scale)));
            }
            Some((json!(if on { "on" } else { "off" }), attrs))
        }
        Kind::Light {
            payload_on,
            payload_off,
            ..
        } => {
            let v = text(&value);
            let on = if v == *payload_on || v.eq_ignore_ascii_case("on") {
                true
            } else if v == *payload_off || v.eq_ignore_ascii_case("off") {
                false
            } else {
                return None;
            };
            Some((json!(if on { "on" } else { "off" }), attrs))
        }
        Kind::Switch {
            state_on,
            state_off,
            ..
        } => {
            let v = text(&value);
            if v == *state_on || v.eq_ignore_ascii_case("on") {
                Some((json!("on"), attrs))
            } else if v == *state_off || v.eq_ignore_ascii_case("off") {
                Some((json!("off"), attrs))
            } else {
                None
            }
        }
        Kind::Binary {
            payload_on,
            payload_off,
        } => {
            let v = text(&value);
            if v == *payload_on || v == "true" {
                Some((json!("on"), attrs))
            } else if v == *payload_off || v == "false" {
                Some((json!("off"), attrs))
            } else {
                None
            }
        }
        Kind::Sensor => {
            let v = match value {
                Value::String(s) => s
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(|n| json!(n))
                    .unwrap_or(Value::String(s)),
                other => other,
            };
            Some((v, attrs))
        }
    }
}

fn scaled(b: u64, scale: u64) -> u64 {
    if scale == 0 || scale == 255 {
        b.min(255)
    } else {
        ((b * 255 + scale / 2) / scale).min(255)
    }
}

/// Ce qu'il faut publier pour une commande : (sujet, message).
pub fn commands(item: &Item, target: &Target) -> Vec<(String, String)> {
    let Some(topic) = item.command_topic.clone() else {
        return Vec::new();
    };
    match &item.kind {
        Kind::Light {
            json: true,
            brightness,
            ..
        } => {
            let mut p = json!({ "state": if target.on { "ON" } else { "OFF" } });
            if let (true, Some(b), Some((_, scale))) = (target.on, target.brightness, brightness) {
                p["brightness"] = json!((u64::from(b) * scale + 127) / 255);
            }
            vec![(topic, p.to_string())]
        }
        Kind::Light {
            payload_on,
            payload_off,
            brightness,
            ..
        } => {
            let mut out = Vec::new();
            match (target.on, target.brightness, brightness) {
                (true, Some(b), Some((Some(bt), scale))) => {
                    out.push((bt.clone(), ((u64::from(b) * scale + 127) / 255).to_string()));
                }
                (true, _, _) => out.push((topic, payload_on.clone())),
                (false, _, _) => out.push((topic, payload_off.clone())),
            }
            out
        }
        Kind::Switch {
            payload_on,
            payload_off,
            ..
        } => vec![(
            topic,
            if target.on {
                payload_on.clone()
            } else {
                payload_off.clone()
            },
        )],
        _ => Vec::new(),
    }
}

struct Discovery {
    registry: RegistryHandle,
    client: AsyncClient,
    items: HashMap<String, Item>,
    /// Sujet → entités qui l'écoutent (état, luminosité, disponibilité).
    by_topic: HashMap<String, Vec<(String, Listen)>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Listen {
    State,
    Brightness,
    Availability,
}

impl Discovery {
    async fn on_config(&mut self, topic: &str, payload: &[u8]) {
        // homeassistant/<type>/[<nœud>/]<objet>/config
        let parts: Vec<&str> = topic.split('/').collect();
        if parts.len() < 4 || parts.last() != Some(&"config") {
            return;
        }
        let component = parts[1];
        let object = parts[parts.len() - 2];
        let key = topic.to_string();
        if payload.is_empty() {
            // Annonce effacée : l'appareil est retiré.
            if let Some(item) = self.items.remove(&key) {
                self.forget(&item.entity_id);
                self.registry.remove(&item.entity_id).await;
            }
            return;
        }
        let Ok(Value::Object(raw)) = serde_json::from_slice::<Value>(payload) else {
            tracing::debug!("découverte MQTT : annonce illisible sur {topic}");
            return;
        };
        let Some(item) = parse_config(component, object, &raw) else {
            return;
        };
        if let Some(old) = self.items.remove(&key) {
            self.forget(&old.entity_id);
        }
        self.registry.upsert(item.meta.clone()).await;
        let mut subscribe = Vec::new();
        let mut listen =
            |t: &Option<String>, what: Listen, by: &mut HashMap<String, Vec<(String, Listen)>>| {
                if let Some(t) = t {
                    by.entry(t.clone())
                        .or_default()
                        .push((item.entity_id.clone(), what));
                    subscribe.push(t.clone());
                }
            };
        listen(&item.state_topic, Listen::State, &mut self.by_topic);
        listen(
            &item.brightness_state_topic,
            Listen::Brightness,
            &mut self.by_topic,
        );
        for (t, _, _) in &item.availability {
            listen(&Some(t.clone()), Listen::Availability, &mut self.by_topic);
        }
        for t in subscribe {
            let _ = self.client.try_subscribe(t, QoS::AtMostOnce);
        }
        tracing::info!("découverte MQTT : {} ({})", item.meta.name, item.entity_id);
        self.items.insert(key, item);
    }

    fn forget(&mut self, entity_id: &str) {
        for list in self.by_topic.values_mut() {
            list.retain(|(e, _)| e != entity_id);
        }
        self.by_topic.retain(|_, l| !l.is_empty());
    }

    async fn on_message(&mut self, topic: &str, payload: &[u8]) {
        let Some(listeners) = self.by_topic.get(topic).cloned() else {
            return;
        };
        let text = String::from_utf8_lossy(payload).to_string();
        for (entity_id, what) in listeners {
            let Some(item) = self
                .items
                .values()
                .find(|i| i.entity_id == entity_id)
                .cloned()
            else {
                continue;
            };
            match what {
                Listen::State => {
                    if let Some((v, attrs)) = state_of(&item, &text) {
                        self.registry.report(&entity_id, v, attrs, true).await;
                    }
                }
                Listen::Brightness => {
                    let raw = match &item.brightness_value_template {
                        Some(t) => render(t, &text).and_then(|v| v.as_f64()),
                        None => text.trim().parse::<f64>().ok(),
                    };
                    if let (
                        Some(b),
                        Kind::Light {
                            brightness: Some((_, scale)),
                            ..
                        },
                    ) = (raw, &item.kind)
                    {
                        let mut attrs = Attributes::new();
                        attrs.insert(
                            "brightness".into(),
                            json!(scaled(b.max(0.0) as u64, *scale)),
                        );
                        let value = self
                            .registry
                            .get(&entity_id)
                            .await
                            .and_then(|e| e.state)
                            .map(|s| s.value)
                            .unwrap_or(json!(if b > 0.0 { "on" } else { "off" }));
                        self.registry.report(&entity_id, value, attrs, true).await;
                    }
                }
                Listen::Availability => {
                    for (t, on, off) in &item.availability {
                        if t == topic {
                            if text == *on {
                                self.registry.set_available(&entity_id, true).await;
                            } else if text == *off {
                                self.registry.set_available(&entity_id, false).await;
                            }
                        }
                    }
                }
            }
        }
    }

    fn command(&self, entity_id: &str, target: &Target) {
        let Some(item) = self.items.values().find(|i| i.entity_id == entity_id) else {
            tracing::warn!("découverte MQTT : {entity_id} inconnue");
            return;
        };
        for (topic, payload) in commands(item, target) {
            if let Err(e) =
                self.client
                    .try_publish(topic, QoS::AtLeastOnce, false, payload.into_bytes())
            {
                tracing::warn!("découverte MQTT : commande {entity_id} non envoyée : {e}");
            }
        }
    }
}

pub async fn run(env: MqttEnv, registry: RegistryHandle, bus: Bus) -> anyhow::Result<()> {
    let mut opts = MqttOptions::new(
        format!("heim-discovery-{}", std::process::id()),
        crate::mdns::resolve(&env.host).await,
        env.port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    opts.set_max_packet_size(1024 * 1024, 64 * 1024);
    if let Some(u) = &env.username {
        opts.set_credentials(u.clone(), env.password.clone().unwrap_or_default());
    }
    let (client, mut eventloop) = AsyncClient::new(opts, 512);
    let mut d = Discovery {
        registry,
        client,
        items: HashMap::new(),
        by_topic: HashMap::new(),
    };
    let mut rx = bus.subscribe();
    let mut connected = false;
    loop {
        tokio::select! {
            ev = eventloop.poll() => match ev {
                Ok(MqttEvent::Incoming(Packet::ConnAck(_))) => {
                    connected = true;
                    d.client.try_subscribe(format!("{PREFIX}/+/+/config"), QoS::AtMostOnce)?;
                    d.client.try_subscribe(format!("{PREFIX}/+/+/+/config"), QoS::AtMostOnce)?;
                    // Après une reconnexion, les sujets d'état aussi.
                    for t in d.by_topic.keys() {
                        let _ = d.client.try_subscribe(t.clone(), QoS::AtMostOnce);
                    }
                    bus.integration_status(INTEGRATION, IntegrationState::Connected, None);
                }
                Ok(MqttEvent::Incoming(Packet::Publish(p))) => {
                    if p.topic.starts_with(PREFIX) && p.topic.ends_with("/config") {
                        d.on_config(&p.topic, &p.payload).await;
                    } else {
                        d.on_message(&p.topic, &p.payload).await;
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    if connected {
                        tracing::warn!("découverte MQTT : connexion perdue ({e})");
                    }
                    connected = false;
                    bus.integration_status(INTEGRATION, IntegrationState::Disconnected, Some(e.to_string()));
                    tokio::time::sleep(Duration::from_secs(3)).await;
                }
            },
            ev = rx.recv() => match ev {
                Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                    d.command(&entity_id, &target);
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn tasmota_switch() {
        let raw = obj(json!({
            "name": "Prise salon", "~": "tasmota_ABC/",
            "stat_t": "~stat/RESULT", "cmd_t": "~cmnd/POWER", "val_tpl": "{{value_json.POWER}}",
            "pl_on": "ON", "pl_off": "OFF", "avty_t": "~tele/LWT", "pl_avail": "Online", "pl_not_avail": "Offline",
            "uniq_id": "ABC_RL_1", "dev": {"ids": ["ABC"], "name": "Prise salon", "mdl": "Sonoff S26", "mf": "Tasmota"}
        }));
        let item = parse_config("switch", "ABC_RL_1", &raw).unwrap();
        assert_eq!(item.entity_id, "switch.mqtt_abc_rl_1");
        assert_eq!(item.state_topic.as_deref(), Some("tasmota_ABC/stat/RESULT"));
        assert_eq!(
            item.command_topic.as_deref(),
            Some("tasmota_ABC/cmnd/POWER")
        );
        assert_eq!(item.availability[0].0, "tasmota_ABC/tele/LWT");
        assert_eq!(item.meta.device_id.as_deref(), Some("mqtt:abc"));
        assert_eq!(item.meta.name, "Prise salon");
        assert_eq!(state_of(&item, r#"{"POWER":"ON"}"#).unwrap().0, json!("on"));
        assert_eq!(
            commands(&item, &Target::off()),
            vec![("tasmota_ABC/cmnd/POWER".to_string(), "OFF".to_string())]
        );
    }

    #[test]
    fn esphome_json_light_and_sensor() {
        let light = obj(json!({
            "name": "Ruban", "schema": "json", "brightness": true,
            "stat_t": "salon/light/ruban/state", "cmd_t": "salon/light/ruban/command",
            "uniq_id": "ESPlightruban", "dev": {"ids": "8c4b14", "name": "salon"}
        }));
        let item = parse_config("light", "ruban", &light).unwrap();
        assert_eq!(item.meta.name, "salon Ruban");
        let (v, a) = state_of(&item, r#"{"state":"ON","brightness":128}"#).unwrap();
        assert_eq!(v, json!("on"));
        assert_eq!(a["brightness"], json!(128));
        let cmd = commands(
            &item,
            &Target {
                on: true,
                brightness: Some(255),
                color_temp: None,
            },
        );
        assert_eq!(cmd[0].1, r#"{"brightness":255,"state":"ON"}"#);

        let sensor = obj(json!({
            "name": "Température", "dev_cla": "temperature", "unit_of_meas": "°C",
            "stat_t": "tasmota/tele/SENSOR", "val_tpl": "{{ value_json['AM2301']['Temperature'] | round(1) }}",
            "uniq_id": "t1"
        }));
        let item = parse_config("sensor", "t1", &sensor).unwrap();
        let (v, _) = state_of(&item, r#"{"AM2301":{"Temperature":21.46,"Humidity":40}}"#).unwrap();
        assert_eq!(v, json!(21.5));
        assert_eq!(item.meta.capabilities["kind"], json!("temperature"));
        assert_eq!(item.meta.capabilities["unit"], json!("°C"));
    }

    #[test]
    fn zigbee2mqtt_announcements_are_ignored() {
        let raw = obj(
            json!({"name": null, "uniq_id": "0x00158d_light_zigbee2mqtt", "cmd_t": "zigbee2mqtt/x/set",
            "o": {"name": "Zigbee2MQTT"}}),
        );
        assert!(parse_config("light", "x", &raw).is_none());
        assert!(parse_config("climate", "x", &obj(json!({"uniq_id": "c"}))).is_none());
    }

    #[test]
    fn templates() {
        assert_eq!(render("{{ value }}", "12"), Some(json!("12")));
        assert_eq!(render("{{ value | float }}", "12.5"), Some(json!(12.5)));
        assert_eq!(
            render("{{ value_json.a.b }}", r#"{"a":{"b":3}}"#),
            Some(json!(3))
        );
        assert_eq!(
            render("{{ value_json['x'][1] }}", r#"{"x":[4,5]}"#),
            Some(json!(5))
        );
        assert_eq!(
            render("{{ value_json.t | int }}", r#"{"t":"21.9"}"#),
            Some(json!(21))
        );
        assert_eq!(render("{% if value == 'ON' %}", "ON"), None);
    }
}
