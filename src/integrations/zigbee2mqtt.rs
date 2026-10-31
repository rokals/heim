//! Zigbee via Zigbee2MQTT.
//!
//! Heim ne parle pas la radio Zigbee lui-même : Zigbee2MQTT (avec la clé Sonoff ZBDongle-E)
//! s'en charge et publie tout en MQTT. Heim lit la liste des appareils
//! (`zigbee2mqtt/bridge/devices`), crée les entités, suit leurs états et envoie
//! les commandes sur `zigbee2mqtt/<appareil>/set`.

use std::collections::HashMap;
use std::time::Duration;

use rumqttc::{AsyncClient, Event as MqttEvent, MqttOptions, Packet, QoS};
use serde_json::{Map, Value, json};
use tokio::sync::broadcast::error::RecvError;

use crate::bus::{Bus, Event, IntegrationState};
use crate::model::{Attributes, Category, Domain, EntityId, EntityMeta, Target, slugify};
use crate::registry::RegistryHandle;

pub const INTEGRATION: &str = "zigbee2mqtt";

#[derive(Debug, Clone)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub base_topic: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Light {
        state: String,
        value_on: Value,
        value_off: Value,
        brightness: Option<(String, u64)>,
        color_temp: Option<(String, u64, u64)>,
    },
    Switch {
        state: String,
        value_on: Value,
        value_off: Value,
    },
    Sensor {
        prop: String,
    },
    Text {
        prop: String,
    },
    Binary {
        prop: String,
        /// Valeur qui veut dire « actif ». Pour `contact`, Zigbee2MQTT déclare déjà
        /// `value_on: false` : « on » = porte ouverte, sans rien inverser.
        value_on: Value,
    },
    /// Sirène. Deux familles : `warning` (norme Zigbee IAS, sans retour d'état)
    /// ou un simple interrupteur `alarm` (beaucoup de sirènes Tuya), qui lui renvoie son état.
    Siren {
        warning: bool,
        alarm: Option<(String, Value, Value)>,
    },
}

/// Durée maximale d'une sirène `warning` par commande ; l'alarme l'arrête avant si besoin.
const SIREN_SECONDS: u64 = 300;

#[derive(Debug, Clone)]
pub struct Mapping {
    pub meta: EntityMeta,
    pub kind: Kind,
}

#[derive(Debug, Clone)]
pub struct Device {
    pub friendly_name: String,
    pub mappings: Vec<Mapping>,
    pub ieee: Option<String>,
    /// Tout ce que l'appareil sait régler (voir `crate::devices`).
    pub controls: Vec<Value>,
    pub info: Map<String, Value>,
}

/// Réglages modifiables d'un appareil, d'après ses « exposes » Zigbee2MQTT.
pub fn controls_from_exposes(exposes: &[Value]) -> Vec<Value> {
    fn one(f: &Value, group: Option<&str>, prefix: Option<&str>, out: &mut Vec<Value>) {
        let name = str_field(f, "name").unwrap_or_default();
        let Some(prop) = property(f) else { return };
        if SKIPPED.contains(&name) {
            return;
        }
        let kind = str_field(f, "type").unwrap_or_default();
        let key = match prefix {
            Some(p) => format!("{p}.{prop}"),
            None => prop.clone(),
        };
        // La couleur d'une lampe : un seul réglage, quel que soit le format de l'appareil.
        if kind == "composite" && matches!(name, "color_xy" | "color_hs") {
            if access_settable(f) {
                out.push(json!({
                    "key": "color", "name": name, "kind": "color", "label": "Couleur",
                    "format": if name == "color_xy" { "xy" } else { "hs" },
                    "group": group, "endpoint": str_field(f, "endpoint"),
                }));
            }
            return;
        }
        if kind == "composite" {
            if let Some(sub) = f.get("features").and_then(Value::as_array) {
                for sf in sub {
                    one(sf, group, Some(&key), out);
                }
            }
            return;
        }
        if !access_settable(f) {
            return;
        }
        let mut c = json!({
            "key": key, "name": name, "kind": kind,
            "label": str_field(f, "label").unwrap_or(name),
            "description": str_field(f, "description"),
            "category": str_field(f, "category"),
            "group": group, "endpoint": str_field(f, "endpoint"),
        });
        let o = c.as_object_mut().expect("objet");
        for k in [
            "values",
            "value_on",
            "value_off",
            "value_toggle",
            "value_min",
            "value_max",
            "value_step",
            "unit",
            "presets",
        ] {
            if let Some(v) = f.get(k) {
                let key = match k {
                    "value_min" => "min",
                    "value_max" => "max",
                    "value_step" => "step",
                    other => other,
                };
                o.insert(key.into(), v.clone());
            }
        }
        out.push(c);
    }
    let mut out = Vec::new();
    for e in exposes {
        match str_field(e, "type") {
            Some(t @ ("light" | "switch" | "lock" | "fan" | "cover" | "climate")) => {
                for f in e
                    .get("features")
                    .and_then(Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or(&[])
                {
                    one(f, Some(t), None, &mut out);
                }
            }
            Some(_) => one(e, None, None, &mut out),
            None => {}
        }
    }
    out
}

/// Valeurs publiées qui n'ont pas d'intérêt pour l'utilisateur.
const SKIPPED: &[&str] = &["linkquality", "update", "update_available", "action"];

/// Valeurs techniques : utiles pour diagnostiquer, pas pour l'usage quotidien.
const DIAGNOSTIC: &[&str] = &[
    "battery",
    "battery_low",
    "battery_voltage",
    "voltage",
    "current",
    "energy",
    "device_temperature",
    "power_outage_count",
];

fn category_of(e: &Value, name: &str) -> Category {
    match str_field(e, "category") {
        Some("diagnostic" | "config") => Category::Diagnostic,
        _ if DIAGNOSTIC.contains(&name) => Category::Diagnostic,
        _ => Category::Primary,
    }
}

fn access_published(e: &Value) -> bool {
    e.get("access")
        .and_then(Value::as_u64)
        .is_none_or(|a| a & 1 == 1)
}

fn access_settable(e: &Value) -> bool {
    e.get("access")
        .and_then(Value::as_u64)
        .is_some_and(|a| a & 2 == 2)
}

fn str_field<'a>(e: &'a Value, key: &str) -> Option<&'a str> {
    e.get(key).and_then(Value::as_str)
}

fn feature<'a>(features: &'a [Value], name: &str) -> Option<&'a Value> {
    features.iter().find(|f| str_field(f, "name") == Some(name))
}

fn property(e: &Value) -> Option<String> {
    str_field(e, "property")
        .or_else(|| str_field(e, "name"))
        .map(str::to_string)
}

/// Transforme `bridge/devices` en appareils et entités Heim. Fonction pure, testée sans broker.
pub fn parse_devices(payload: &Value) -> Vec<Device> {
    let Some(list) = payload.as_array() else {
        return Vec::new();
    };
    let mut used_ids: HashMap<String, usize> = HashMap::new();
    let mut devices = Vec::new();

    for dev in list {
        if str_field(dev, "type") == Some("Coordinator")
            || dev.get("disabled").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let (Some(friendly), Some(definition)) =
            (str_field(dev, "friendly_name"), dev.get("definition"))
        else {
            continue;
        };
        let Some(exposes) = definition.get("exposes").and_then(Value::as_array) else {
            continue;
        };
        let ieee = str_field(dev, "ieee_address").map(str::to_string);
        let slug = slugify(friendly);
        let mut capabilities_base = Attributes::new();
        if let Some(m) = str_field(definition, "model") {
            capabilities_base.insert("model".into(), json!(m));
        }
        if let Some(v) = str_field(definition, "vendor") {
            capabilities_base.insert("manufacturer".into(), json!(v));
        }
        capabilities_base.insert("device_name".into(), json!(friendly));

        let mut mappings = Vec::new();
        let mut make = |domain: Domain,
                        suffix: Option<&str>,
                        name: String,
                        kind: Kind,
                        extra: Attributes,
                        category: Category| {
            let base = match suffix {
                Some(s) if !s.is_empty() => {
                    format!("{}.{}_{}", domain.prefix(), slug, slugify(s))
                }
                _ => format!("{}.{}", domain.prefix(), slug),
            };
            let n = used_ids.entry(base.clone()).or_insert(0);
            *n += 1;
            let entity_id = if *n == 1 { base } else { format!("{base}_{n}") };
            let mut capabilities = capabilities_base.clone();
            capabilities.extend(extra);
            mappings.push(Mapping {
                meta: EntityMeta {
                    entity_id,
                    domain,
                    name,
                    integration: INTEGRATION.into(),
                    device_id: ieee.clone(),
                    room: None,
                    plan: None,
                    capabilities,
                    user_edited: false,
                    category,
                    hidden: false,
                },
                kind,
            });
        };

        for e in exposes {
            let endpoint = str_field(e, "endpoint");
            let label = |base: &str| match endpoint {
                Some(ep) => format!("{base} {ep}"),
                None => base.to_string(),
            };
            match str_field(e, "type") {
                Some(t @ ("light" | "switch")) => {
                    let features = e
                        .get("features")
                        .and_then(Value::as_array)
                        .map(Vec::as_slice)
                        .unwrap_or(&[]);
                    let Some(state) = feature(features, "state") else {
                        continue;
                    };
                    let Some(state_prop) = property(state) else {
                        continue;
                    };
                    let value_on = state.get("value_on").cloned().unwrap_or(json!("ON"));
                    let value_off = state.get("value_off").cloned().unwrap_or(json!("OFF"));
                    if t == "light" {
                        let brightness = feature(features, "brightness").and_then(|f| {
                            Some((
                                property(f)?,
                                f.get("value_max").and_then(Value::as_u64).unwrap_or(254),
                            ))
                        });
                        let color_temp = feature(features, "color_temp").and_then(|f| {
                            Some((
                                property(f)?,
                                f.get("value_min").and_then(Value::as_u64).unwrap_or(150),
                                f.get("value_max").and_then(Value::as_u64).unwrap_or(500),
                            ))
                        });
                        let mut extra = Attributes::new();
                        if brightness.is_some() {
                            extra.insert("brightness".into(), json!(true));
                        }
                        if let Some((_, min, max)) = &color_temp {
                            extra.insert("color_temp".into(), json!([min, max]));
                        }
                        make(
                            Domain::Light,
                            endpoint,
                            label(friendly),
                            Kind::Light {
                                state: state_prop,
                                value_on,
                                value_off,
                                brightness,
                                color_temp,
                            },
                            extra,
                            Category::Primary,
                        );
                    } else {
                        make(
                            Domain::Switch,
                            endpoint,
                            label(friendly),
                            Kind::Switch {
                                state: state_prop,
                                value_on,
                                value_off,
                            },
                            Attributes::new(),
                            Category::Primary,
                        );
                    }
                }
                Some("numeric") => {
                    let name = str_field(e, "name").unwrap_or_default();
                    if SKIPPED.contains(&name) || !access_published(e) || access_settable(e) {
                        continue;
                    }
                    let Some(prop) = property(e) else { continue };
                    let mut extra = Attributes::new();
                    if let Some(u) = str_field(e, "unit") {
                        extra.insert("unit".into(), json!(u));
                    }
                    extra.insert("kind".into(), json!(name));
                    let human = str_field(e, "label")
                        .map(str::to_string)
                        .unwrap_or_else(|| name.replace('_', " "));
                    make(
                        Domain::Sensor,
                        Some(&prop.clone()),
                        format!("{friendly} {human}"),
                        Kind::Sensor { prop },
                        extra,
                        category_of(e, name),
                    );
                }
                Some("composite") if str_field(e, "name") == Some("warning") => {
                    make(
                        Domain::Siren,
                        None,
                        friendly.to_string(),
                        Kind::Siren {
                            warning: true,
                            alarm: None,
                        },
                        Attributes::new(),
                        Category::Primary,
                    );
                }
                Some("binary") if str_field(e, "name") == Some("alarm") && access_settable(e) => {
                    let Some(prop) = property(e) else { continue };
                    let on = e.get("value_on").cloned().unwrap_or(json!(true));
                    let off = e.get("value_off").cloned().unwrap_or(json!(false));
                    make(
                        Domain::Siren,
                        None,
                        friendly.to_string(),
                        Kind::Siren {
                            warning: false,
                            alarm: Some((prop, on, off)),
                        },
                        Attributes::new(),
                        Category::Primary,
                    );
                }
                Some("binary") => {
                    let name = str_field(e, "name").unwrap_or_default();
                    if SKIPPED.contains(&name) || !access_published(e) || access_settable(e) {
                        continue;
                    }
                    let Some(prop) = property(e) else { continue };
                    let value_on = e.get("value_on").cloned().unwrap_or(json!(true));
                    let mut extra = Attributes::new();
                    extra.insert("kind".into(), json!(name));
                    let human = str_field(e, "label")
                        .map(str::to_string)
                        .unwrap_or_else(|| name.replace('_', " "));
                    make(
                        Domain::BinarySensor,
                        Some(&prop.clone()),
                        format!("{friendly} {human}"),
                        Kind::Binary { prop, value_on },
                        extra,
                        category_of(e, name),
                    );
                }
                Some("enum" | "text") => {
                    // Valeurs textuelles en lecture seule : état de présence d'un radar, mode d'un appareil...
                    let name = str_field(e, "name").unwrap_or_default();
                    if SKIPPED.contains(&name) || !access_published(e) || access_settable(e) {
                        continue;
                    }
                    let Some(prop) = property(e) else { continue };
                    let mut extra = Attributes::new();
                    extra.insert("kind".into(), json!(name));
                    let human = str_field(e, "label")
                        .map(str::to_string)
                        .unwrap_or_else(|| name.replace('_', " "));
                    make(
                        Domain::Sensor,
                        Some(&prop.clone()),
                        format!("{friendly} {human}"),
                        Kind::Text { prop },
                        extra,
                        category_of(e, name),
                    );
                }
                _ => {}
            }
        }
        if !mappings.is_empty() {
            let mut info = Map::new();
            for (k, src) in [
                ("model", "model"),
                ("vendor", "vendor"),
                ("description", "description"),
            ] {
                if let Some(v) = definition.get(src).filter(|v| !v.is_null()) {
                    info.insert(k.into(), v.clone());
                }
            }
            for k in [
                "power_source",
                "software_build_id",
                "date_code",
                "type",
                "manufacturer",
                "model_id",
            ] {
                if let Some(v) = dev.get(k).filter(|v| !v.is_null()) {
                    info.insert(k.into(), v.clone());
                }
            }
            info.insert("friendly_name".into(), json!(friendly));
            devices.push(Device {
                friendly_name: friendly.to_string(),
                mappings,
                ieee: ieee.clone(),
                controls: controls_from_exposes(exposes),
                info,
            });
        }
    }
    devices
}

/// Extrait l'état d'une entité depuis un message d'état de l'appareil.
/// `None` si le message ne parle pas de cette entité.
pub fn extract(kind: &Kind, payload: &Map<String, Value>) -> Option<(Value, Attributes)> {
    match kind {
        Kind::Light {
            state,
            value_on,
            value_off,
            brightness,
            color_temp,
        } => {
            let v = payload.get(state)?;
            let on = if v == value_on {
                true
            } else if v == value_off {
                false
            } else {
                return None;
            };
            let mut attrs = Attributes::new();
            if let Some((prop, max)) = brightness
                && let Some(b) = payload.get(prop).and_then(Value::as_u64)
            {
                // Zigbee2MQTT va de 0 à 254 : on ramène sur 0..255.
                let scaled = if *max == 255 || *max == 0 {
                    b
                } else {
                    (b * 255 + max / 2) / max
                };
                attrs.insert("brightness".into(), json!(scaled.min(255)));
            }
            if let Some((prop, _, _)) = color_temp
                && let Some(ct) = payload.get(prop).and_then(Value::as_u64)
            {
                attrs.insert("color_temp".into(), json!(ct));
            }
            Some((json!(if on { "on" } else { "off" }), attrs))
        }
        Kind::Switch {
            state,
            value_on,
            value_off,
        } => {
            let v = payload.get(state)?;
            let on = if v == value_on {
                true
            } else if v == value_off {
                false
            } else {
                return None;
            };
            Some((json!(if on { "on" } else { "off" }), Attributes::new()))
        }
        Kind::Sensor { prop } => {
            let v = payload.get(prop)?;
            if !v.is_number() {
                return None;
            }
            Some((v.clone(), Attributes::new()))
        }
        Kind::Siren {
            alarm: Some((prop, on, _)),
            ..
        } => {
            let v = payload.get(prop)?;
            Some((json!(if v == on { "on" } else { "off" }), Attributes::new()))
        }
        Kind::Siren { alarm: None, .. } => None,
        Kind::Text { prop } => {
            let v = payload.get(prop)?;
            if !(v.is_string() || v.is_number() || v.is_boolean()) {
                return None;
            }
            Some((v.clone(), Attributes::new()))
        }
        Kind::Binary { prop, value_on } => {
            let v = payload.get(prop)?;
            if v.is_null() {
                return None;
            }
            let on = v == value_on;
            Some((json!(if on { "on" } else { "off" }), Attributes::new()))
        }
    }
}

/// Construit le message `/set` pour une cible.
pub fn command_payload(kind: &Kind, target: &Target) -> Option<Value> {
    let mut out = Map::new();
    match kind {
        Kind::Light {
            state,
            value_on,
            value_off,
            brightness,
            color_temp,
        } => {
            out.insert(
                state.clone(),
                if target.on {
                    value_on.clone()
                } else {
                    value_off.clone()
                },
            );
            if target.on {
                if let (Some(b), Some((prop, max))) = (target.brightness, brightness) {
                    let scaled = (b as u64 * max + 127) / 255;
                    out.insert(prop.clone(), json!(scaled.max(1)));
                }
                if let (Some(ct), Some((prop, min, max))) = (target.color_temp, color_temp) {
                    out.insert(prop.clone(), json!((ct as u64).clamp(*min, *max)));
                }
            }
        }
        Kind::Switch {
            state,
            value_on,
            value_off,
        } => {
            out.insert(
                state.clone(),
                if target.on {
                    value_on.clone()
                } else {
                    value_off.clone()
                },
            );
        }
        Kind::Siren {
            alarm: Some((prop, on, off)),
            ..
        } => {
            out.insert(
                prop.clone(),
                if target.on { on.clone() } else { off.clone() },
            );
        }
        Kind::Siren {
            warning: true,
            alarm: None,
        } => {
            let warning = if target.on {
                json!({ "mode": "burglar", "level": "very_high", "strobe": true, "duration": SIREN_SECONDS })
            } else {
                json!({ "mode": "stop", "level": "low", "strobe": false, "duration": 0 })
            };
            out.insert("warning".into(), warning);
        }
        Kind::Siren { .. } | Kind::Sensor { .. } | Kind::Text { .. } | Kind::Binary { .. } => {
            return None;
        }
    }
    Some(Value::Object(out))
}

fn parse_online(payload: &[u8]) -> Option<bool> {
    let text = std::str::from_utf8(payload).ok()?.trim();
    let state = match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(o)) => o.get("state").and_then(Value::as_str).map(str::to_string)?,
        Ok(Value::String(s)) => s,
        _ => text.to_string(),
    };
    match state.as_str() {
        "online" => Some(true),
        "offline" => Some(false),
        _ => None,
    }
}

struct Bridge {
    cfg: Config,
    client: AsyncClient,
    registry: RegistryHandle,
    devices: HashMap<String, Device>,
    /// entity_id -> (nom de l'appareil, index du mapping)
    by_entity: HashMap<EntityId, (String, usize)>,
    bus: Bus,
    /// Version majeure de Zigbee2MQTT : le message d'appairage a changé en 2.0.
    major_version: Option<u32>,
    controls: crate::devices::Controls,
}

/// Message pour ouvrir ou fermer l'appairage, selon la version de Zigbee2MQTT.
/// 2.x : `{"time": 60}` (0 ferme) ; 1.x : `{"value": true, "time": 60}`.
pub fn permit_join_payload(seconds: u32, major_version: Option<u32>) -> Value {
    let seconds = seconds.min(254);
    match major_version {
        Some(v) if v < 2 => json!({ "value": seconds > 0, "time": seconds }),
        _ => json!({ "time": seconds }),
    }
}

/// Lit `bridge/info` : appairage ouvert ? jusqu'à quand ? quelle version ?
pub fn parse_bridge_info(
    info: &Value,
) -> (bool, Option<chrono::DateTime<chrono::Utc>>, Option<String>) {
    let open = info
        .get("permit_join")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let version = info
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_string);
    let until = if !open {
        None
    } else if let Some(end) = info.get("permit_join_end").and_then(Value::as_i64) {
        chrono::DateTime::from_timestamp_millis(end)
    } else {
        info.get("permit_join_timeout")
            .and_then(Value::as_i64)
            .map(|s| chrono::Utc::now() + chrono::TimeDelta::seconds(s))
    };
    (open, until, version)
}

pub async fn run(
    cfg: Config,
    registry: RegistryHandle,
    bus: Bus,
    controls: crate::devices::Controls,
) -> anyhow::Result<()> {
    let mut opts = MqttOptions::new(
        format!("heim-{}", std::process::id()),
        crate::mdns::resolve(&cfg.host).await,
        cfg.port,
    );
    opts.set_keep_alive(Duration::from_secs(30));
    // La liste des appareils peut peser plusieurs centaines de Ko : la limite par défaut (10 Ko) ne suffit pas.
    opts.set_max_packet_size(4 * 1024 * 1024, 256 * 1024);
    if let Some(user) = &cfg.username {
        opts.set_credentials(user.clone(), cfg.password.clone().unwrap_or_default());
    }
    let (client, mut eventloop) = AsyncClient::new(opts, 256);
    let mut bus_rx = bus.subscribe();
    let mut bridge = Bridge {
        cfg,
        client,
        registry,
        devices: HashMap::new(),
        by_entity: HashMap::new(),
        bus: bus.clone(),
        major_version: None,
        controls,
    };
    let mut connected = false;
    // Broker joint mais aucun signe de Zigbee2MQTT : on le dit au bout de quelques secondes.
    let mut waiting_bridge: Option<tokio::time::Instant> = None;
    // On prévient une fois clairement, puis on réessaie sans inonder le terminal.
    let mut warned = false;
    let mut backoff = Duration::from_secs(1);

    tracing::info!(
        "zigbee2mqtt : connexion à {}:{}",
        bridge.cfg.host,
        bridge.cfg.port
    );
    loop {
        tokio::select! {
            ev = eventloop.poll() => match ev {
                Ok(MqttEvent::Incoming(Packet::ConnAck(_))) => {
                    connected = true;
                    warned = false;
                    backoff = Duration::from_secs(1);
                    let topic = format!("{}/#", bridge.cfg.base_topic);
                    bridge.client.try_subscribe(topic, QoS::AtMostOnce)?;
                    bus.integration_status(INTEGRATION, IntegrationState::Connected, None);
                    tracing::info!("zigbee2mqtt : connecté au broker MQTT");
                    waiting_bridge = Some(tokio::time::Instant::now() + Duration::from_secs(12));
                }
                Ok(MqttEvent::Incoming(Packet::Publish(p))) => {
                    if p.topic.ends_with("/bridge/state") || p.topic.ends_with("/bridge/devices") {
                        waiting_bridge = None;
                    }
                    if let Some(request) = p.topic.strip_prefix(&format!("{}/bridge/response/", bridge.cfg.base_topic)) {
                        let v: Value = serde_json::from_slice(&p.payload).unwrap_or_default();
                        let ok = v.get("status").and_then(Value::as_str) == Some("ok");
                        let error = v.get("error").and_then(Value::as_str).map(str::to_string);
                        if !ok {
                            tracing::warn!("zigbee2mqtt : {request} refusé : {}", error.clone().unwrap_or_default());
                        }
                        bus.publish(Event::ZigbeeResponse { request: request.to_string(), ok, error });
                    }
                    if p.topic == format!("{}/bridge/state", bridge.cfg.base_topic) {
                        if let Some(online) = parse_online(&p.payload) {
                            bus.publish(Event::ZigbeeBridgeOnline { online });
                        }
                        match parse_online(&p.payload) {
                            Some(true) => bus.integration_status(INTEGRATION, IntegrationState::Connected, None),
                            Some(false) => bus.integration_status(
                                INTEGRATION,
                                IntegrationState::Disconnected,
                                Some("Zigbee2MQTT est arrêté".into()),
                            ),
                            None => {}
                        }
                    }
                    bridge.on_publish(&p.topic, &p.payload).await
                }
                Ok(_) => {}
                Err(e) => {
                    if connected {
                        tracing::warn!("zigbee2mqtt : connexion perdue ({e}), reconnexion automatique");
                        bus.integration_status(INTEGRATION, IntegrationState::Disconnected, Some(e.to_string()));
                        bridge.mark_all(false).await;
                    } else if !warned {
                        warned = true;
                        tracing::warn!(
                            "zigbee2mqtt : impossible de joindre le broker MQTT {}:{} ({e}). \
                             Vérifiez l'adresse (IP seule, sans http://), le port 1883 et les identifiants. \
                             Nouvel essai automatique.",
                            bridge.cfg.host, bridge.cfg.port
                        );
                        bus.integration_status(INTEGRATION, IntegrationState::Disconnected, Some(e.to_string()));
                    } else {
                        tracing::debug!("zigbee2mqtt : broker injoignable ({e}), nouvel essai dans {backoff:?}");
                        bus.integration_status(INTEGRATION, IntegrationState::Disconnected, Some(e.to_string()));
                    }
                    connected = false;
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(15));
                }
            },
            _ = tokio::time::sleep_until(waiting_bridge.unwrap_or_else(tokio::time::Instant::now)), if waiting_bridge.is_some() => {
                waiting_bridge = None;
                bus.integration_status(
                    INTEGRATION,
                    IntegrationState::Connected,
                    Some("broker joint, mais Zigbee2MQTT ne répond pas".into()),
                );
            }
            ev = bus_rx.recv() => match ev {
                Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                    bridge.send_command(&entity_id, &target).await;
                }
                Ok(Event::ZigbeePermitJoin { seconds }) => bridge.permit_join(seconds),
                Ok(Event::DeviceSet { integration, device_id, values }) if integration == INTEGRATION => {
                    bridge.set_values(&device_id, &values);
                }
                Ok(Event::DeviceAction { integration, device_id, action, force }) if integration == INTEGRATION => {
                    bridge.device_action(&device_id, &action, force);
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
        }
    }
}

impl Bridge {
    async fn on_publish(&mut self, topic: &str, payload: &[u8]) {
        let Some(rest) = topic
            .strip_prefix(&self.cfg.base_topic)
            .and_then(|r| r.strip_prefix('/'))
        else {
            return;
        };
        if rest == "bridge/devices" {
            match serde_json::from_slice::<Value>(payload) {
                Ok(v) => self.load_devices(parse_devices(&v)).await,
                Err(e) => tracing::warn!("zigbee2mqtt : liste d'appareils illisible : {e}"),
            }
        } else if rest == "bridge/state" {
            if parse_online(payload) == Some(false) {
                tracing::warn!("zigbee2mqtt : le pont Zigbee2MQTT est hors ligne");
                self.mark_all(false).await;
            }
        } else if rest == "bridge/info" {
            if let Ok(info) = serde_json::from_slice::<Value>(payload) {
                let (open, until, version) = parse_bridge_info(&info);
                self.major_version = version
                    .as_deref()
                    .and_then(|v| v.split('.').next()?.parse().ok());
                self.bus.publish(Event::ZigbeeBridge {
                    permit_join: open,
                    until,
                    version,
                });
            }
        } else if rest == "bridge/event" {
            if let Ok(ev) = serde_json::from_slice::<Value>(payload) {
                let kind = ev
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let data = ev.get("data").cloned().unwrap_or_default();
                let name = data
                    .get("friendly_name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let detail = match kind.as_str() {
                    "device_interview" => {
                        let status = data
                            .get("status")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let model = data
                            .pointer("/definition/description")
                            .or_else(|| data.pointer("/definition/model"))
                            .and_then(Value::as_str);
                        Some(match model {
                            Some(m) if status == "successful" => format!("{status} : {m}"),
                            _ => status.to_string(),
                        })
                    }
                    _ => None,
                };
                if !kind.is_empty() {
                    self.bus.publish(Event::ZigbeeDevice {
                        event: kind,
                        friendly_name: name,
                        detail,
                    });
                }
            }
        } else if rest.starts_with("bridge/") || rest.ends_with("/set") || rest.ends_with("/get") {
            // Messages internes ou nos propres commandes.
        } else if let Some(name) = rest.strip_suffix("/availability") {
            if let Some(online) = parse_online(payload) {
                self.mark_device(name, online).await;
            }
        } else if let Some(device) = self.devices.get(rest) {
            let Ok(Value::Object(obj)) = serde_json::from_slice::<Value>(payload) else {
                return;
            };
            if let Some(id) = &device.ieee
                && let Some(d) = crate::devices::lock(&self.controls).get_mut(id)
            {
                for (k, v) in &obj {
                    d.values.insert(k.clone(), v.clone());
                }
            }
            // La qualité du signal vaut pour tout l'appareil.
            let mut technical = Attributes::new();
            if let Some(lq) = obj.get("linkquality").filter(|v| v.is_number()) {
                technical.insert("linkquality".into(), lq.clone());
            }
            let updates: Vec<(EntityId, Option<(Value, Attributes)>)> = device
                .mappings
                .iter()
                .map(|m| (m.meta.entity_id.clone(), extract(&m.kind, &obj)))
                .collect();
            for (id, update) in updates {
                match update {
                    Some((value, mut attrs)) => {
                        attrs.extend(technical.clone());
                        self.registry.report(id, value, attrs, true).await;
                    }
                    None => self.registry.seen(id, technical.clone()).await,
                }
            }
        }
    }

    async fn load_devices(&mut self, devices: Vec<Device>) {
        let count: usize = devices.iter().map(|d| d.mappings.len()).sum();
        tracing::info!("zigbee2mqtt : {} appareils, {count} entités", devices.len());
        self.devices.clear();
        self.by_entity.clear();
        {
            let mut map = crate::devices::lock(&self.controls);
            map.retain(|_, d| d.integration != INTEGRATION);
            for device in &devices {
                if let Some(id) = &device.ieee {
                    map.insert(
                        id.clone(),
                        crate::devices::DeviceControls {
                            integration: INTEGRATION.into(),
                            device_id: id.clone(),
                            name: device.friendly_name.clone(),
                            controls: device.controls.clone(),
                            values: Map::new(),
                            info: device.info.clone(),
                        },
                    );
                }
            }
        }
        for device in devices {
            for (i, m) in device.mappings.iter().enumerate() {
                self.by_entity
                    .insert(m.meta.entity_id.clone(), (device.friendly_name.clone(), i));
                self.registry.upsert(m.meta.clone()).await;
                // Une sirène « warning » n'a pas d'état lisible : silencieuse jusqu'à preuve du contraire.
                if matches!(m.kind, Kind::Siren { alarm: None, .. })
                    && self
                        .registry
                        .get(&m.meta.entity_id)
                        .await
                        .and_then(|e| e.state)
                        .is_none()
                {
                    self.registry
                        .report(
                            m.meta.entity_id.clone(),
                            json!("off"),
                            Attributes::new(),
                            true,
                        )
                        .await;
                }
            }
            // Demande l'état actuel des lumières et prises (les capteurs sur pile ne répondent pas).
            if device
                .mappings
                .iter()
                .any(|m| matches!(m.kind, Kind::Light { .. } | Kind::Switch { .. }))
            {
                let topic = format!("{}/{}/get", self.cfg.base_topic, device.friendly_name);
                if let Err(e) = self.client.try_publish(
                    topic,
                    QoS::AtMostOnce,
                    false,
                    br#"{"state":""}"#.to_vec(),
                ) {
                    tracing::debug!("zigbee2mqtt : demande d'état impossible : {e}");
                }
            }
            self.devices.insert(device.friendly_name.clone(), device);
        }
    }

    fn by_ieee(&self, ieee: &str) -> Option<&Device> {
        self.devices
            .values()
            .find(|d| d.ieee.as_deref() == Some(ieee))
    }

    /// Réglages libres : `{"power_on_behavior": "previous"}`, `{"color": {"hex": "#ff8800"}}`...
    fn set_values(&self, ieee: &str, values: &Map<String, Value>) {
        let Some(device) = self.by_ieee(ieee) else {
            tracing::warn!("zigbee2mqtt : appareil {ieee} inconnu");
            return;
        };
        let payload = Value::Object(crate::devices::nest(values));
        let topic = format!("{}/{}/set", self.cfg.base_topic, device.friendly_name);
        if let Err(e) = self.client.try_publish(
            topic,
            QoS::AtLeastOnce,
            false,
            payload.to_string().into_bytes(),
        ) {
            tracing::warn!("zigbee2mqtt : réglage non envoyé : {e}");
        }
    }

    /// Actions sur l'appareil lui-même : retirer du réseau, chercher une mise à jour.
    fn device_action(&self, ieee: &str, action: &str, force: bool) {
        let Some(device) = self.by_ieee(ieee) else {
            return;
        };
        // Relire la pile : demande directe à l'appareil ({base}/{nom}/get). Un appareil sur pile
        // qui dort ne répond qu'à son réveil (appui sur son bouton, prochain message).
        if action == "read_battery" {
            let topic = format!("{}/{}/get", self.cfg.base_topic, device.friendly_name);
            let payload = json!({ "battery": "" });
            if let Err(e) = self.client.try_publish(
                topic,
                QoS::AtLeastOnce,
                false,
                payload.to_string().into_bytes(),
            ) {
                tracing::warn!("zigbee2mqtt : lecture de la pile non envoyée : {e}");
            }
            return;
        }
        let (topic, payload) = match action {
            "remove" => (
                "device/remove",
                json!({ "id": device.friendly_name, "force": force }),
            ),
            "ota_check" => (
                "device/ota_update/check",
                json!({ "id": device.friendly_name }),
            ),
            "ota_update" => (
                "device/ota_update/update",
                json!({ "id": device.friendly_name }),
            ),
            "interview" => ("device/interview", json!({ "id": device.friendly_name })),
            _ => return,
        };
        let topic = format!("{}/bridge/request/{topic}", self.cfg.base_topic);
        if let Err(e) = self.client.try_publish(
            topic,
            QoS::AtLeastOnce,
            false,
            payload.to_string().into_bytes(),
        ) {
            tracing::warn!("zigbee2mqtt : action {action} non envoyée : {e}");
        }
    }

    fn permit_join(&self, seconds: u32) {
        let topic = format!("{}/bridge/request/permit_join", self.cfg.base_topic);
        let payload = permit_join_payload(seconds, self.major_version);
        match self.client.try_publish(
            topic,
            QoS::AtLeastOnce,
            false,
            payload.to_string().into_bytes(),
        ) {
            Ok(()) => tracing::info!(
                "zigbee2mqtt : appairage {}",
                if seconds > 0 { "ouvert" } else { "fermé" }
            ),
            Err(e) => tracing::warn!("zigbee2mqtt : appairage impossible : {e}"),
        }
    }

    async fn send_command(&self, entity_id: &str, target: &Target) {
        let Some((name, idx)) = self.by_entity.get(entity_id) else {
            tracing::warn!("zigbee2mqtt : {entity_id} inconnue du pont (appareil retiré ?)");
            return;
        };
        let Some(mapping) = self.devices.get(name).and_then(|d| d.mappings.get(*idx)) else {
            return;
        };
        let Some(payload) = command_payload(&mapping.kind, target) else {
            return;
        };
        let topic = format!("{}/{}/set", self.cfg.base_topic, name);
        if let Err(e) = self.client.try_publish(
            topic,
            QoS::AtLeastOnce,
            false,
            payload.to_string().into_bytes(),
        ) {
            tracing::warn!("zigbee2mqtt : commande {entity_id} non envoyée : {e}");
            return;
        }
        // Les sirènes « warning » ne disent jamais si elles sonnent : on note l'état demandé.
        if matches!(mapping.kind, Kind::Siren { alarm: None, .. }) {
            self.registry
                .report(
                    entity_id,
                    json!(if target.on { "on" } else { "off" }),
                    Attributes::new(),
                    true,
                )
                .await;
            if target.on {
                // La sirène se tait d'elle-même au bout de SIREN_SECONDS : on suit.
                let registry = self.registry.clone();
                let id = entity_id.to_string();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(SIREN_SECONDS + 1)).await;
                    if registry
                        .get(&id)
                        .await
                        .and_then(|e| e.state)
                        .is_some_and(|s| s.value == json!("on"))
                    {
                        registry
                            .report(id, json!("off"), Attributes::new(), true)
                            .await;
                    }
                });
            }
        }
    }

    async fn mark_device(&self, name: &str, online: bool) {
        let Some(device) = self.devices.get(name) else {
            return;
        };
        for m in &device.mappings {
            self.registry
                .set_available(m.meta.entity_id.clone(), online)
                .await;
        }
    }

    async fn mark_all(&self, online: bool) {
        for id in self.by_entity.keys() {
            self.registry.set_available(id.clone(), online).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!([
            {"type": "Coordinator", "friendly_name": "Coordinator", "ieee_address": "0x00"},
            {
                "type": "Router", "friendly_name": "Salon/Plafonnier", "ieee_address": "0x01",
                "definition": {"model": "LED1545G12", "vendor": "IKEA", "exposes": [
                    {"type": "light", "features": [
                        {"type": "binary", "name": "state", "property": "state", "value_on": "ON", "value_off": "OFF", "access": 7},
                        {"type": "numeric", "name": "brightness", "property": "brightness", "value_min": 0, "value_max": 254, "access": 7},
                        {"type": "numeric", "name": "color_temp", "property": "color_temp", "value_min": 250, "value_max": 454, "access": 7}
                    ]},
                    {"type": "numeric", "name": "linkquality", "property": "linkquality", "access": 1}
                ]}
            },
            {
                "type": "Router", "friendly_name": "Prise double", "ieee_address": "0x02",
                "definition": {"model": "TS011F", "vendor": "TuYa", "exposes": [
                    {"type": "switch", "endpoint": "l1", "features": [{"type": "binary", "name": "state", "property": "state_l1", "value_on": "ON", "value_off": "OFF", "access": 7}]},
                    {"type": "switch", "endpoint": "l2", "features": [{"type": "binary", "name": "state", "property": "state_l2", "value_on": "ON", "value_off": "OFF", "access": 7}]},
                    {"type": "numeric", "name": "power", "property": "power", "unit": "W", "access": 5},
                    {"type": "enum", "name": "power_on_behavior", "property": "power_on_behavior", "access": 7}
                ]}
            },
            {
                "type": "EndDevice", "friendly_name": "Porte entrée", "ieee_address": "0x03",
                "definition": {"model": "MCCGQ11LM", "vendor": "Aqara", "exposes": [
                    {"type": "binary", "name": "contact", "property": "contact", "value_on": false, "value_off": true, "access": 1},
                    {"type": "numeric", "name": "battery", "property": "battery", "unit": "%", "access": 1},
                    {"type": "numeric", "name": "temperature", "property": "temperature", "unit": "°C", "access": 1}
                ]}
            },
            {"type": "EndDevice", "friendly_name": "Inconnu", "ieee_address": "0x04", "definition": null}
        ])
    }

    #[test]
    fn builds_controls_from_exposes() {
        let exposes = json!([
            {"type": "light", "features": [
                {"type": "binary", "name": "state", "property": "state", "value_on": "ON", "value_off": "OFF", "value_toggle": "TOGGLE", "access": 7},
                {"type": "numeric", "name": "brightness", "property": "brightness", "value_min": 0, "value_max": 254, "access": 7},
                {"type": "numeric", "name": "color_temp", "property": "color_temp", "value_min": 153, "value_max": 500, "access": 7,
                 "presets": [{"name": "coolest", "value": 153}, {"name": "warmest", "value": 500}]},
                {"type": "composite", "name": "color_xy", "property": "color", "access": 7, "features": [
                    {"type": "numeric", "name": "x", "property": "x", "access": 7}, {"type": "numeric", "name": "y", "property": "y", "access": 7}]}
            ]},
            {"type": "enum", "name": "effect", "property": "effect", "access": 2, "values": ["blink", "breathe", "okay", "stop_effect"]},
            {"type": "enum", "name": "power_on_behavior", "label": "Power-on behavior", "property": "power_on_behavior", "access": 7,
             "values": ["off", "on", "toggle", "previous"], "category": "config"},
            {"type": "composite", "name": "level_config", "property": "level_config", "access": 7, "features": [
                {"type": "numeric", "name": "on_level", "property": "on_level", "value_min": 1, "value_max": 254, "access": 7}]},
            {"type": "numeric", "name": "linkquality", "property": "linkquality", "access": 1},
            {"type": "numeric", "name": "power", "property": "power", "unit": "W", "access": 5}
        ]);
        let c = controls_from_exposes(exposes.as_array().unwrap());
        let keys: Vec<&str> = c.iter().map(|x| x["key"].as_str().unwrap()).collect();
        assert_eq!(
            keys,
            vec![
                "state",
                "brightness",
                "color_temp",
                "color",
                "effect",
                "power_on_behavior",
                "level_config.on_level"
            ]
        );
        assert_eq!(c[0]["group"], json!("light"));
        assert_eq!(c[1]["max"], json!(254));
        assert_eq!(c[3]["kind"], json!("color"));
        assert_eq!(c[3]["format"], json!("xy"));
        assert_eq!(c[5]["category"], json!("config"));
        let d = crate::devices::DeviceControls {
            controls: c,
            ..Default::default()
        };
        assert_eq!(
            crate::devices::power_on_supported(&d),
            vec!["previous", "on", "off"]
        );
    }

    #[test]
    fn parses_zigbee_sirens() {
        let devices = parse_devices(&json!([
            {
                "type": "Router", "friendly_name": "Sirène garage", "ieee_address": "0x10",
                "definition": {"model": "HS2WD-E", "vendor": "Heiman", "exposes": [
                    {"type": "composite", "name": "warning", "property": "warning", "access": 2, "features": [
                        {"type": "enum", "name": "mode", "property": "mode", "access": 2, "values": ["stop", "burglar"]}
                    ]},
                    {"type": "numeric", "name": "battery", "property": "battery", "unit": "%", "access": 1}
                ]}
            },
            {
                "type": "Router", "friendly_name": "Sirène tuya", "ieee_address": "0x11",
                "definition": {"model": "TS0601_siren", "vendor": "TuYa", "exposes": [
                    {"type": "binary", "name": "alarm", "property": "alarm", "value_on": true, "value_off": false, "access": 3}
                ]}
            }
        ]));
        let heiman = &devices[0].mappings[0];
        assert_eq!(heiman.meta.domain, Domain::Siren);
        assert!(matches!(
            heiman.kind,
            Kind::Siren {
                warning: true,
                alarm: None
            }
        ));
        let on = command_payload(
            &heiman.kind,
            &Target {
                on: true,
                brightness: None,
                color_temp: None,
            },
        )
        .unwrap();
        assert_eq!(on["warning"]["mode"], json!("burglar"));
        let off = command_payload(
            &heiman.kind,
            &Target {
                on: false,
                brightness: None,
                color_temp: None,
            },
        )
        .unwrap();
        assert_eq!(off["warning"]["mode"], json!("stop"));

        let tuya = &devices[1].mappings[0];
        assert_eq!(tuya.meta.domain, Domain::Siren);
        let on = command_payload(
            &tuya.kind,
            &Target {
                on: true,
                brightness: None,
                color_temp: None,
            },
        )
        .unwrap();
        assert_eq!(on, json!({"alarm": true}));
    }

    #[test]
    fn permit_join_depends_on_version() {
        assert_eq!(permit_join_payload(120, Some(2)), json!({"time": 120}));
        assert_eq!(permit_join_payload(0, None), json!({"time": 0}));
        assert_eq!(permit_join_payload(999, Some(2)), json!({"time": 254}));
        assert_eq!(
            permit_join_payload(60, Some(1)),
            json!({"value": true, "time": 60})
        );
        assert_eq!(
            permit_join_payload(0, Some(1)),
            json!({"value": false, "time": 0})
        );

        let (open, until, version) = parse_bridge_info(
            &json!({"version": "2.1.0", "permit_join": true, "permit_join_end": 1_900_000_000_000_i64}),
        );
        assert!(open);
        assert_eq!(until.unwrap().timestamp(), 1_900_000_000);
        assert_eq!(version.as_deref(), Some("2.1.0"));
        let (open, until, _) =
            parse_bridge_info(&json!({"version": "1.40.0", "permit_join": false}));
        assert!(!open && until.is_none());
    }

    #[test]
    fn parses_realistic_bridge_devices() {
        let devices = parse_devices(&sample());
        let ids: Vec<&str> = devices
            .iter()
            .flat_map(|d| d.mappings.iter().map(|m| m.meta.entity_id.as_str()))
            .collect();
        assert_eq!(
            ids,
            vec![
                "light.salon_plafonnier",
                "switch.prise_double_l1",
                "switch.prise_double_l2",
                "sensor.prise_double_power",
                "binary_sensor.porte_entree_contact",
                "sensor.porte_entree_battery",
                "sensor.porte_entree_temperature",
            ]
        );
        let light = &devices[0].mappings[0];
        assert_eq!(light.meta.capabilities["manufacturer"], json!("IKEA"));
        assert_eq!(
            light.meta.capabilities["device_name"],
            json!("Salon/Plafonnier")
        );
        let battery = &devices[2].mappings[1];
        assert_eq!(battery.meta.entity_id, "sensor.porte_entree_battery");
        assert_eq!(battery.meta.category, Category::Diagnostic);
        assert_eq!(devices[2].mappings[0].meta.category, Category::Primary);
        assert_eq!(light.meta.capabilities["color_temp"], json!([250, 454]));
    }

    #[test]
    fn extracts_and_scales_brightness() {
        let devices = parse_devices(&sample());
        let kind = &devices[0].mappings[0].kind;
        let msg = json!({"state": "ON", "brightness": 254, "color_temp": 300, "linkquality": 90});
        let (v, attrs) = extract(kind, msg.as_object().unwrap()).unwrap();
        assert_eq!(v, json!("on"));
        assert_eq!(attrs["brightness"], json!(255));
        assert_eq!(attrs["color_temp"], json!(300));
    }

    #[test]
    fn contact_on_means_open() {
        let devices = parse_devices(&sample());
        let kind = &devices[2].mappings[0].kind;
        let closed = json!({"contact": true});
        assert_eq!(
            extract(kind, closed.as_object().unwrap()).unwrap().0,
            json!("off")
        );
        let open = json!({"contact": false});
        assert_eq!(
            extract(kind, open.as_object().unwrap()).unwrap().0,
            json!("on")
        );
    }

    #[test]
    fn command_payloads() {
        let devices = parse_devices(&sample());
        let light = &devices[0].mappings[0].kind;
        let p = command_payload(
            light,
            &Target {
                on: true,
                brightness: Some(255),
                color_temp: Some(100),
            },
        )
        .unwrap();
        assert_eq!(
            p,
            json!({"state": "ON", "brightness": 254, "color_temp": 250})
        );
        let p = command_payload(light, &Target::off()).unwrap();
        assert_eq!(p, json!({"state": "OFF"}));
        let plug = &devices[1].mappings[1].kind;
        assert_eq!(
            command_payload(plug, &Target::on()).unwrap(),
            json!({"state_l2": "ON"})
        );
    }

    #[test]
    fn availability_formats() {
        assert_eq!(parse_online(br#"{"state":"online"}"#), Some(true));
        assert_eq!(parse_online(b"offline"), Some(false));
        assert_eq!(parse_online(b"???"), None);
    }
}
