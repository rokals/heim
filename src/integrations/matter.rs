//! Matter, par l'intermédiaire d'un serveur Matter (python-matter-server, ou le module
//! « Matter Server » de Home Assistant, port 5580).
//!
//! Heim ne parle pas Matter lui-même (chiffrement, certificats, Bluetooth, Thread : c'est le
//! travail du serveur). Il se branche sur son API WebSocket : liste des appareils (« nœuds »),
//! attributs en direct, commandes, appairage par code.
//!
//! Chemins d'attributs : « point/cluster/attribut », par exemple « 1/6/0 » = marche/arrêt du
//! point 1.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio_tungstenite::tungstenite::Message;

use crate::bus::{Bus, Event, IntegrationState};
use crate::model::{Attributes, Category, Domain, EntityMeta, Target};
use crate::registry::RegistryHandle;

pub const INTEGRATION: &str = "matter";
pub const SETTINGS_KEY: &str = "matter";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatterSettings {
    #[serde(default)]
    pub enabled: bool,
    /// `ws://192.168.1.50:5580/ws`
    #[serde(default)]
    pub url: String,
}

/// Accepte « 192.168.1.50 », « 192.168.1.50:5580 », « ws://…/ws ».
pub fn normalize_url(raw: &str) -> String {
    let s = raw.trim().trim_end_matches('/');
    let s = s
        .strip_prefix("http://")
        .map(|r| format!("ws://{r}"))
        .or_else(|| s.strip_prefix("https://").map(|r| format!("wss://{r}")))
        .unwrap_or_else(|| s.to_string());
    let with_scheme = if s.starts_with("ws://") || s.starts_with("wss://") {
        s
    } else {
        format!("ws://{s}")
    };
    let host_part = with_scheme
        .split('/')
        .nth(2)
        .unwrap_or_default()
        .to_string();
    let mut url = with_scheme.clone();
    if !host_part.contains(':') {
        url = url.replacen(&host_part, &format!("{host_part}:5580"), 1);
    }
    if !url
        .trim_start_matches("ws://")
        .trim_start_matches("wss://")
        .contains('/')
    {
        url.push_str("/ws");
    }
    url
}

// ---- Clusters et types d'appareils ------------------------------------------------------

mod cl {
    pub const BASIC: u32 = 40;
    pub const BRIDGED: u32 = 57;
    pub const DESCRIPTOR: u32 = 29;
    pub const ON_OFF: u32 = 6;
    pub const LEVEL: u32 = 8;
    pub const COLOR: u32 = 768;
    pub const BOOLEAN: u32 = 69;
    pub const POWER_SOURCE: u32 = 47;
    pub const ILLUMINANCE: u32 = 1024;
    pub const TEMPERATURE: u32 = 1026;
    pub const HUMIDITY: u32 = 1029;
    pub const OCCUPANCY: u32 = 1030;
    pub const ELECTRICAL_POWER: u32 = 144;
}

const LIGHT_TYPES: [u64; 4] = [0x0100, 0x0101, 0x010C, 0x010D];
const PLUG_TYPES: [u64; 3] = [0x010A, 0x010B, 0x0103];
const WATER_LEAK: u64 = 0x0043;

#[derive(Debug, Clone, PartialEq)]
pub enum Role {
    Light {
        dimmable: bool,
        color_temp: bool,
    },
    Switch,
    Sensor {
        cluster: u32,
        attr: u32,
    },
    Binary {
        cluster: u32,
        inverted: bool,
        bitmap: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Mapped {
    pub meta: EntityMeta,
    pub node_id: u64,
    pub endpoint: u32,
    pub role: Role,
}

type Attrs = serde_json::Map<String, Value>;

fn attr(a: &Attrs, ep: u32, cluster: u32, id: u32) -> Option<&Value> {
    a.get(&format!("{ep}/{cluster}/{id}"))
        .filter(|v| !v.is_null())
}

fn has_cluster(a: &Attrs, ep: u32, cluster: u32) -> bool {
    let p = format!("{ep}/{cluster}/");
    a.keys().any(|k| k.starts_with(&p))
}

fn endpoints(a: &Attrs) -> Vec<u32> {
    let mut eps: Vec<u32> = a
        .keys()
        .filter_map(|k| k.split('/').next()?.parse().ok())
        .filter(|ep| *ep != 0)
        .collect();
    eps.sort_unstable();
    eps.dedup();
    eps
}

fn device_types(a: &Attrs, ep: u32) -> Vec<u64> {
    attr(a, ep, cl::DESCRIPTOR, 0)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|d| {
                    d.get("0")
                        .or_else(|| d.get("deviceType"))
                        .and_then(Value::as_u64)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn text(a: &Attrs, ep: u32, cluster: u32, id: u32) -> Option<String> {
    attr(a, ep, cluster, id)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Les entités Heim d'un nœud Matter.
pub fn map_node(node: &Value) -> Vec<Mapped> {
    let Some(node_id) = node.get("node_id").and_then(Value::as_u64) else {
        return Vec::new();
    };
    let empty = Attrs::new();
    let a = node
        .get("attributes")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let vendor = text(a, 0, cl::BASIC, 1);
    let product = text(a, 0, cl::BASIC, 3);
    let node_name = text(a, 0, cl::BASIC, 5)
        .or_else(|| product.clone())
        .unwrap_or_else(|| format!("Matter {node_id}"));
    let mut out = Vec::new();
    let eps = endpoints(a);
    for ep in eps.iter().copied() {
        let types = device_types(a, ep);
        let name = text(a, ep, cl::BRIDGED, 5)
            .or_else(|| text(a, ep, cl::BRIDGED, 3))
            .unwrap_or_else(|| node_name.clone());
        let many = eps.len() > 1 && text(a, ep, cl::BRIDGED, 5).is_none();
        let base = format!("matter_{node_id}_{ep}");
        let mut caps = Attributes::new();
        caps.insert("device_name".into(), json!(name));
        if let Some(v) = &vendor {
            caps.insert("manufacturer".into(), json!(v));
        }
        if let Some(p) = &product {
            caps.insert("model".into(), json!(p));
        }
        let device_id = if text(a, ep, cl::BRIDGED, 5).is_some() {
            format!("matter:{node_id}:{ep}")
        } else {
            format!("matter:{node_id}")
        };
        let mut push = |domain: Domain,
                        suffix: &str,
                        label: String,
                        extra: Attributes,
                        role: Role,
                        category: Category| {
            let mut capabilities = caps.clone();
            capabilities.extend(extra);
            out.push(Mapped {
                meta: EntityMeta {
                    entity_id: if suffix.is_empty() {
                        format!("{}.{base}", domain.prefix())
                    } else {
                        format!("{}.{base}_{suffix}", domain.prefix())
                    },
                    domain,
                    name: label,
                    integration: INTEGRATION.into(),
                    device_id: Some(device_id.clone()),
                    room: None,
                    plan: None,
                    capabilities,
                    user_edited: false,
                    category,
                    hidden: false,
                },
                node_id,
                endpoint: ep,
                role,
            });
        };
        let label = |what: &str| {
            if many {
                format!("{name} {ep}")
            } else if what.is_empty() {
                name.clone()
            } else {
                format!("{name} {what}")
            }
        };

        if has_cluster(a, ep, cl::ON_OFF) && !types.contains(&0x0103) {
            let is_plug = types.iter().any(|t| PLUG_TYPES.contains(t));
            let is_light = types.iter().any(|t| LIGHT_TYPES.contains(t))
                || (!is_plug && has_cluster(a, ep, cl::LEVEL));
            if is_light {
                let dimmable = has_cluster(a, ep, cl::LEVEL);
                let color_temp = attr(a, ep, cl::COLOR, 7).is_some();
                let mut extra = Attributes::new();
                if dimmable {
                    extra.insert("brightness".into(), json!(true));
                }
                if color_temp {
                    let min = attr(a, ep, cl::COLOR, 16395)
                        .and_then(Value::as_u64)
                        .unwrap_or(153);
                    let max = attr(a, ep, cl::COLOR, 16396)
                        .and_then(Value::as_u64)
                        .unwrap_or(500);
                    extra.insert("color_temp".into(), json!([min, max]));
                }
                push(
                    Domain::Light,
                    "",
                    label(""),
                    extra,
                    Role::Light {
                        dimmable,
                        color_temp,
                    },
                    Category::Primary,
                );
            } else {
                push(
                    Domain::Switch,
                    "",
                    label(""),
                    Attributes::new(),
                    Role::Switch,
                    Category::Primary,
                );
            }
        }
        for (cluster, attr_id, kind, unit, what) in [
            (cl::TEMPERATURE, 0, "temperature", "°C", "température"),
            (cl::HUMIDITY, 0, "humidity", "%", "humidité"),
            (cl::ILLUMINANCE, 0, "illuminance", "lx", "luminosité"),
            (cl::ELECTRICAL_POWER, 8, "power", "W", "puissance"),
        ] {
            if attr(a, ep, cluster, attr_id).is_some()
                || has_cluster(a, ep, cluster) && cluster != cl::ELECTRICAL_POWER
            {
                let mut extra = Attributes::new();
                extra.insert("kind".into(), json!(kind));
                extra.insert("unit".into(), json!(unit));
                push(
                    Domain::Sensor,
                    kind,
                    label(what),
                    extra,
                    Role::Sensor {
                        cluster,
                        attr: attr_id,
                    },
                    Category::Primary,
                );
            }
        }
        if attr(a, ep, cl::POWER_SOURCE, 12).is_some() {
            let mut extra = Attributes::new();
            extra.insert("kind".into(), json!("battery"));
            extra.insert("unit".into(), json!("%"));
            push(
                Domain::Sensor,
                "battery",
                label("pile"),
                extra,
                Role::Sensor {
                    cluster: cl::POWER_SOURCE,
                    attr: 12,
                },
                Category::Diagnostic,
            );
        }
        if has_cluster(a, ep, cl::BOOLEAN) {
            let leak = types.contains(&WATER_LEAK);
            let mut extra = Attributes::new();
            extra.insert(
                "kind".into(),
                json!(if leak { "water_leak" } else { "contact" }),
            );
            // Contact : « vrai » = fermé. Heim : « on » = ouvert.
            push(
                Domain::BinarySensor,
                if leak { "leak" } else { "contact" },
                label(if leak { "fuite" } else { "ouverture" }),
                extra,
                Role::Binary {
                    cluster: cl::BOOLEAN,
                    inverted: !leak,
                    bitmap: false,
                },
                Category::Primary,
            );
        }
        if has_cluster(a, ep, cl::OCCUPANCY) {
            let mut extra = Attributes::new();
            extra.insert("kind".into(), json!("occupancy"));
            push(
                Domain::BinarySensor,
                "occupancy",
                label("présence"),
                extra,
                Role::Binary {
                    cluster: cl::OCCUPANCY,
                    inverted: false,
                    bitmap: true,
                },
                Category::Primary,
            );
        }
    }
    out
}

/// L'état d'une entité d'après les attributs du nœud.
pub fn state_of(m: &Mapped, a: &Attrs) -> Option<(Value, Attributes)> {
    let ep = m.endpoint;
    match &m.role {
        Role::Switch => attr(a, ep, cl::ON_OFF, 0)?
            .as_bool()
            .map(|on| (json!(if on { "on" } else { "off" }), Attributes::new())),
        Role::Light {
            dimmable,
            color_temp,
        } => {
            let on = attr(a, ep, cl::ON_OFF, 0)?.as_bool()?;
            let mut attrs = Attributes::new();
            if *dimmable && let Some(l) = attr(a, ep, cl::LEVEL, 0).and_then(Value::as_u64) {
                attrs.insert(
                    "brightness".into(),
                    json!(l.clamp(1, 254) as u8 + u8::from(l >= 254)),
                );
            }
            if *color_temp && let Some(ct) = attr(a, ep, cl::COLOR, 7).and_then(Value::as_u64) {
                attrs.insert("color_temp".into(), json!(ct));
            }
            Some((json!(if on { "on" } else { "off" }), attrs))
        }
        Role::Sensor { cluster, attr: id } => {
            let raw = attr(a, ep, *cluster, *id)?.as_f64()?;
            let v = match *cluster {
                cl::TEMPERATURE | cl::HUMIDITY => raw / 100.0,
                cl::POWER_SOURCE => raw / 2.0,
                cl::ELECTRICAL_POWER => raw / 1000.0,
                cl::ILLUMINANCE => {
                    if raw <= 0.0 {
                        0.0
                    } else {
                        10f64.powf((raw - 1.0) / 10000.0)
                    }
                }
                _ => raw,
            };
            Some((json!((v * 10.0).round() / 10.0), Attributes::new()))
        }
        Role::Binary {
            cluster,
            inverted,
            bitmap,
        } => {
            let v = attr(a, ep, *cluster, 0)?;
            let active = if *bitmap {
                v.as_u64().map(|b| b & 1 == 1)?
            } else {
                v.as_bool()?
            };
            let on = active != *inverted;
            Some((json!(if on { "on" } else { "off" }), Attributes::new()))
        }
    }
}

/// Les commandes Matter (cluster, nom, paramètres) pour une cible Heim.
pub fn commands_for(m: &Mapped, target: &Target) -> Vec<(u32, &'static str, Value)> {
    match &m.role {
        Role::Switch => vec![(cl::ON_OFF, if target.on { "On" } else { "Off" }, json!({}))],
        Role::Light {
            dimmable,
            color_temp,
        } => {
            if !target.on {
                return vec![(cl::ON_OFF, "Off", json!({}))];
            }
            let mut v = Vec::new();
            if let (true, Some(ct)) = (*color_temp, target.color_temp) {
                v.push((
                    cl::COLOR,
                    "MoveToColorTemperature",
                    json!({ "colorTemperatureMireds": ct, "transitionTime": 0, "optionsMask": 0, "optionsOverride": 0 }),
                ));
            }
            match (*dimmable, target.brightness) {
                (true, Some(b)) => v.push((
                    cl::LEVEL,
                    "MoveToLevelWithOnOff",
                    json!({ "level": b.clamp(1, 254), "transitionTime": 0, "optionsMask": 0, "optionsOverride": 0 }),
                )),
                _ => v.push((cl::ON_OFF, "On", json!({}))),
            }
            v
        }
        _ => Vec::new(),
    }
}

// ---- Connexion au serveur -----------------------------------------------------------------

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(url: &str) -> Result<(Ws, Value), String> {
    let (mut ws, _) = tokio::time::timeout(
        Duration::from_secs(8),
        tokio_tungstenite::connect_async(url),
    )
    .await
    .map_err(|_| format!("{url} ne répond pas"))?
    .map_err(|e| format!("connexion à {url} impossible : {e}"))?;
    // Le serveur se présente d'abord (version, Wi-Fi/Thread configurés, Bluetooth...).
    let info = match tokio::time::timeout(Duration::from_secs(5), ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str(&t).unwrap_or(Value::Null),
        _ => return Err("le serveur Matter ne s'est pas présenté".into()),
    };
    Ok((ws, info))
}

/// Une commande ponctuelle sur une connexion à part (appairage, retrait d'un appareil).
pub async fn request(
    url: &str,
    command: &str,
    args: Value,
    timeout: Duration,
) -> Result<Value, String> {
    let (mut ws, _) = connect(url).await?;
    let msg = json!({ "message_id": "heim-1", "command": command, "args": args });
    ws.send(Message::Text(msg.to_string().into()))
        .await
        .map_err(|e| e.to_string())?;
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Err("le serveur Matter n'a pas répondu à temps".into()),
            m = ws.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    let v: Value = serde_json::from_str(&t).unwrap_or(Value::Null);
                    if v.get("message_id").and_then(Value::as_str) != Some("heim-1") {
                        continue;
                    }
                    if let Some(code) = v.get("error_code") {
                        let details = v.get("details").and_then(Value::as_str).unwrap_or("erreur");
                        return Err(format!("{details} (code {code})"));
                    }
                    return Ok(v.get("result").cloned().unwrap_or(Value::Null));
                }
                Some(Ok(_)) => {}
                _ => return Err("connexion au serveur Matter coupée".into()),
            },
        }
    }
}

/// Infos du serveur pour la page Réglages.
pub async fn server_info(url: &str) -> Result<Value, String> {
    let (mut ws, info) = connect(url).await?;
    let _ = ws.close(None).await;
    Ok(info)
}

struct Bridge {
    registry: RegistryHandle,
    nodes: HashMap<u64, (Attrs, Vec<Mapped>, bool)>,
    by_entity: HashMap<String, (u64, usize)>,
    next_id: u64,
}

impl Bridge {
    async fn node(&mut self, node: &Value) {
        let Some(id) = node.get("node_id").and_then(Value::as_u64) else {
            return;
        };
        let attrs = node
            .get("attributes")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let available = node
            .get("available")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let mapped = map_node(node);
        for (i, m) in mapped.iter().enumerate() {
            self.by_entity.insert(m.meta.entity_id.clone(), (id, i));
            self.registry.upsert(m.meta.clone()).await;
        }
        self.nodes.insert(id, (attrs, mapped, available));
        self.publish(id).await;
    }

    async fn publish(&self, id: u64) {
        let Some((attrs, mapped, available)) = self.nodes.get(&id) else {
            return;
        };
        for m in mapped {
            match state_of(m, attrs) {
                Some((v, a)) => {
                    self.registry
                        .report(m.meta.entity_id.clone(), v, a, *available)
                        .await
                }
                None => {
                    self.registry
                        .set_available(m.meta.entity_id.clone(), *available)
                        .await
                }
            }
        }
    }

    async fn attribute(&mut self, data: &Value) {
        let (Some(id), Some(path)) = (
            data.get(0).and_then(Value::as_u64),
            data.get(1).and_then(Value::as_str),
        ) else {
            return;
        };
        let value = data.get(2).cloned().unwrap_or(Value::Null);
        if let Some((attrs, _, _)) = self.nodes.get_mut(&id) {
            attrs.insert(path.to_string(), value);
        }
        self.publish(id).await;
    }

    fn command_messages(&mut self, entity_id: &str, target: &Target) -> Vec<String> {
        let Some((node, i)) = self.by_entity.get(entity_id).copied() else {
            return Vec::new();
        };
        let Some(m) = self.nodes.get(&node).and_then(|n| n.1.get(i)).cloned() else {
            return Vec::new();
        };
        commands_for(&m, target)
            .into_iter()
            .map(|(cluster, name, payload)| {
                self.next_id += 1;
                json!({
                    "message_id": format!("cmd-{}", self.next_id),
                    "command": "device_command",
                    "args": { "node_id": node, "endpoint_id": m.endpoint, "cluster_id": cluster, "command_name": name, "payload": payload },
                })
                .to_string()
            })
            .collect()
    }
}

pub async fn run(
    settings: MatterSettings,
    registry: RegistryHandle,
    bus: Bus,
) -> anyhow::Result<()> {
    let url = normalize_url(&settings.url);
    bus.integration_status(INTEGRATION, IntegrationState::Starting, None);
    let (mut ws, info) = match connect(&url).await {
        Ok(c) => c,
        Err(e) => {
            bus.integration_status(INTEGRATION, IntegrationState::Disconnected, Some(e.clone()));
            anyhow::bail!(e);
        }
    };
    let sdk = info
        .get("sdk_version")
        .and_then(Value::as_str)
        .unwrap_or("?");
    ws.send(Message::Text(
        json!({ "message_id": "listen", "command": "start_listening" })
            .to_string()
            .into(),
    ))
    .await?;
    let mut bridge = Bridge {
        registry,
        nodes: HashMap::new(),
        by_entity: HashMap::new(),
        next_id: 0,
    };
    let mut bus_rx = bus.subscribe();
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            m = ws.next() => {
                let text = match m {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Ping(p))) => { let _ = ws.send(Message::Pong(p)).await; continue; }
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => anyhow::bail!("serveur Matter : {e}"),
                    None => anyhow::bail!("serveur Matter : connexion fermée"),
                };
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                if v.get("message_id").and_then(Value::as_str) == Some("listen") {
                    let nodes = v.get("result").and_then(Value::as_array).cloned().unwrap_or_default();
                    for n in &nodes {
                        bridge.node(n).await;
                    }
                    bus.integration_status(
                        INTEGRATION,
                        IntegrationState::Connected,
                        Some(format!("{} appareil(s), SDK {sdk}", nodes.len())),
                    );
                    continue;
                }
                if let Some(code) = v.get("error_code") {
                    let details = v.get("details").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                    tracing::warn!("matter : commande refusée ({code}) : {details}");
                    continue;
                }
                match v.get("event").and_then(Value::as_str) {
                    Some("attribute_updated") => bridge.attribute(v.get("data").unwrap_or(&Value::Null)).await,
                    Some("node_added") | Some("node_updated") => {
                        if let Some(n) = v.get("data") { bridge.node(n).await; }
                    }
                    Some("node_removed") => {
                        if let Some(id) = v.get("data").and_then(Value::as_u64)
                            && let Some((_, mapped, _)) = bridge.nodes.remove(&id) {
                                for m in mapped { bridge.registry.set_available(m.meta.entity_id, false).await; }
                            }
                    }
                    Some("server_shutdown") => anyhow::bail!("le serveur Matter s'arrête"),
                    _ => {}
                }
            }
            ev = bus_rx.recv() => match ev {
                Ok(Event::CommandRequested { integration, entity_id, target, .. }) if integration == INTEGRATION => {
                    for msg in bridge.command_messages(&entity_id, &target) {
                        ws.send(Message::Text(msg.into())).await?;
                    }
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Ok(()),
            },
            _ = ping.tick() => { ws.send(Message::Ping(Vec::new().into())).await?; }
        }
    }
}

/// Les nœuds connus, pour la page Réglages (nom, modèle, disponibilité).
pub fn summarize(nodes: &[Value]) -> Vec<Value> {
    nodes
        .iter()
        .map(|n| {
            let mapped = map_node(n);
            json!({
                "node_id": n.get("node_id"),
                "available": n.get("available"),
                "name": mapped.first().and_then(|m| m.meta.capabilities.get("device_name").cloned()),
                "entities": mapped.len(),
            })
        })
        .collect::<Vec<_>>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulb() -> Value {
        json!({
            "node_id": 4, "available": true,
            "attributes": {
                "0/40/1": "IKEA of Sweden", "0/40/3": "KAJPLATS E27", "0/40/5": "",
                "1/29/0": [{"0": 268, "1": 4}],
                "1/6/0": true, "1/8/0": 127, "1/768/7": 370, "1/768/16395": 153, "1/768/16396": 454
            }
        })
    }

    #[test]
    fn urls_are_forgiving() {
        assert_eq!(normalize_url("192.168.1.50"), "ws://192.168.1.50:5580/ws");
        assert_eq!(
            normalize_url("ws://homeassistant.local:5580/ws"),
            "ws://homeassistant.local:5580/ws"
        );
        assert_eq!(
            normalize_url("http://192.168.1.50:5580"),
            "ws://192.168.1.50:5580/ws"
        );
    }

    #[test]
    fn maps_a_color_temperature_bulb() {
        let m = map_node(&bulb());
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].meta.entity_id, "light.matter_4_1");
        assert_eq!(m[0].meta.name, "KAJPLATS E27");
        assert_eq!(m[0].meta.capabilities["color_temp"], json!([153, 454]));
        let a = bulb()["attributes"].as_object().unwrap().clone();
        let (v, attrs) = state_of(&m[0], &a).unwrap();
        assert_eq!(v, json!("on"));
        assert_eq!(attrs["brightness"], json!(127));
        assert_eq!(attrs["color_temp"], json!(370));
        let cmds = commands_for(
            &m[0],
            &Target {
                on: true,
                brightness: Some(200),
                color_temp: None,
            },
        );
        assert_eq!(cmds[0].1, "MoveToLevelWithOnOff");
        assert_eq!(cmds[0].2["level"], json!(200));
        assert_eq!(
            commands_for(
                &m[0],
                &Target {
                    on: false,
                    brightness: None,
                    color_temp: None
                }
            )[0]
            .1,
            "Off"
        );
    }

    #[test]
    fn maps_plug_and_sensors() {
        let plug = json!({"node_id": 7, "attributes": {"0/40/3": "Eve Energy", "1/29/0": [{"0": 266, "1": 1}], "1/6/0": false}});
        let m = map_node(&plug);
        assert_eq!(m[0].meta.domain, Domain::Switch);
        let sensor = json!({"node_id": 9, "attributes": {
            "0/40/3": "Capteur", "1/29/0": [{"0": 21, "1": 1}], "1/69/0": true,
            "2/29/0": [{"0": 770, "1": 1}], "2/1026/0": 2153, "1/47/12": 180
        }});
        let m = map_node(&sensor);
        let a = sensor["attributes"].as_object().unwrap().clone();
        let contact = m
            .iter()
            .find(|x| x.meta.domain == Domain::BinarySensor)
            .unwrap();
        assert_eq!(
            state_of(contact, &a).unwrap().0,
            json!("off"),
            "contact fermé"
        );
        let temp = m
            .iter()
            .find(|x| x.meta.entity_id.ends_with("temperature"))
            .unwrap();
        assert_eq!(state_of(temp, &a).unwrap().0, json!(21.5));
        let bat = m
            .iter()
            .find(|x| x.meta.entity_id.ends_with("battery"))
            .unwrap();
        assert_eq!(state_of(bat, &a).unwrap().0, json!(90.0));
    }
}
