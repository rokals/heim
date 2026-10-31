//! Reprendre une maison qui tourne sous Home Assistant.
//!
//! Le plus précieux, ce sont les appareils Zigbee : ils sont appairés à la clé, et la clé est
//! décrite par les fichiers de Zigbee2MQTT (clé réseau, base des appareils, sauvegarde du
//! coordinateur). Zigbee2MQTT sait livrer ces fichiers lui-même : on lui demande sa
//! sauvegarde (`bridge/request/backup`) et il répond par un zip. Deux façons de le joindre :
//!
//! - **par Home Assistant**, avec un jeton d'accès longue durée : Heim passe par l'API
//!   WebSocket de HA (publication et abonnement MQTT), lit au passage les pièces (zones) de
//!   chaque appareil, et peut arrêter le module Zigbee2MQTT de HA d'un clic ;
//! - **par le broker MQTT** directement (adresse, utilisateur, mot de passe).
//!
//! Ensuite : clé débranchée de HA, branchée sur la box, Zigbee2MQTT démarre avec les mêmes
//! fichiers et les appareils sont là, sans rien réappairer.

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

pub const ROOMS_KEY: &str = "migrate.rooms";
const MAX_ZIP: usize = 64 * 1024 * 1024;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// « homeassistant.local:8123 », « http://192.168.1.10:8123/ » → adresse WebSocket.
pub fn ws_url(input: &str) -> Result<String, String> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("indiquez l'adresse de Home Assistant".into());
    }
    if s.starts_with("https://") || s.starts_with("wss://") {
        return Err("adresse https non prise en charge : utilisez l'adresse locale de Home Assistant (http://…:8123)".into());
    }
    let host = s
        .trim_start_matches("http://")
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap_or("");
    if host.is_empty() || host.contains(' ') {
        return Err("adresse de Home Assistant invalide".into());
    }
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:8123")
    };
    Ok(format!("ws://{host}/api/websocket"))
}

pub struct Ha {
    ws: Ws,
    next: u64,
    pub version: Option<String>,
}

impl Ha {
    pub async fn connect(url: &str, token: &str) -> Result<Ha, String> {
        let url = ws_url(url)?;
        let (mut ws, _) = tokio::time::timeout(
            Duration::from_secs(10),
            tokio_tungstenite::connect_async(&url),
        )
        .await
        .map_err(|_| "Home Assistant ne répond pas (adresse, port 8123 ?)".to_string())?
        .map_err(|e| format!("Home Assistant injoignable : {e}"))?;
        let mut version = None;
        loop {
            let m = tokio::time::timeout(Duration::from_secs(10), ws.next())
                .await
                .map_err(|_| "Home Assistant ne répond pas".to_string())?;
            let Some(Ok(Message::Text(t))) = m else {
                return Err("connexion à Home Assistant coupée".into());
            };
            let v: Value = serde_json::from_str(&t).unwrap_or_default();
            match v.get("type").and_then(Value::as_str) {
                Some("auth_required") => {
                    version = v
                        .get("ha_version")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    let auth = json!({ "type": "auth", "access_token": token.trim() });
                    ws.send(Message::Text(auth.to_string().into()))
                        .await
                        .map_err(|e| e.to_string())?;
                }
                Some("auth_ok") => break,
                Some("auth_invalid") => {
                    return Err(
                        "jeton refusé par Home Assistant : recréez un jeton d'accès longue durée"
                            .into(),
                    );
                }
                _ => {}
            }
        }
        Ok(Ha {
            ws,
            next: 1,
            version,
        })
    }

    async fn send(&mut self, mut msg: Value) -> Result<u64, String> {
        let id = self.next;
        self.next += 1;
        msg["id"] = json!(id);
        self.ws
            .send(Message::Text(msg.to_string().into()))
            .await
            .map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// Une commande et son résultat.
    pub async fn call(&mut self, msg: Value) -> Result<Value, String> {
        let id = self.send(msg).await?;
        loop {
            let m = tokio::time::timeout(Duration::from_secs(30), self.ws.next())
                .await
                .map_err(|_| "Home Assistant n'a pas répondu à temps".to_string())?;
            let Some(Ok(Message::Text(t))) = m else {
                if matches!(m, Some(Ok(_))) {
                    continue;
                }
                return Err("connexion à Home Assistant coupée".into());
            };
            let v: Value = serde_json::from_str(&t).unwrap_or_default();
            if v.get("id").and_then(Value::as_u64) != Some(id)
                || v.get("type").and_then(Value::as_str) != Some("result")
            {
                continue;
            }
            if v.get("success").and_then(Value::as_bool) == Some(true) {
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            let msg = v
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("erreur");
            return Err(format!("Home Assistant : {msg}"));
        }
    }

    /// La sauvegarde de Zigbee2MQTT, demandée par MQTT au travers de Home Assistant.
    pub async fn z2m_backup(&mut self, base_topic: &str) -> Result<Vec<u8>, String> {
        let sub = self
            .send(json!({ "type": "mqtt/subscribe", "topic": format!("{base_topic}/bridge/response/backup") }))
            .await?;
        // Le résultat de l'abonnement, puis la demande.
        let mut subscribed = false;
        let mut asked = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
        loop {
            let m = tokio::time::timeout_at(deadline, self.ws.next())
                .await
                .map_err(|_| "Zigbee2MQTT n'a pas envoyé sa sauvegarde : est-il démarré dans Home Assistant ?".to_string())?;
            let Some(Ok(msg)) = m else {
                return Err("connexion à Home Assistant coupée".into());
            };
            let Message::Text(t) = msg else { continue };
            let v: Value = serde_json::from_str(&t).unwrap_or_default();
            let id = v.get("id").and_then(Value::as_u64);
            match v.get("type").and_then(Value::as_str) {
                Some("result") if id == Some(sub) => {
                    if v.get("success").and_then(Value::as_bool) != Some(true) {
                        return Err("Home Assistant refuse l'abonnement MQTT : l'intégration MQTT est-elle installée, et le jeton est-il celui d'un administrateur ?".into());
                    }
                    subscribed = true;
                }
                Some("event") if id == Some(sub) => {
                    let payload = v
                        .pointer("/event/payload")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    return backup_from_response(payload);
                }
                _ => {}
            }
            if subscribed && !asked {
                asked = true;
                self.send(json!({
                    "type": "call_service", "domain": "mqtt", "service": "publish",
                    "service_data": { "topic": format!("{base_topic}/bridge/request/backup"), "payload": "{}" }
                }))
                .await?;
            }
        }
    }

    /// Pièces de Home Assistant, par adresse IEEE des appareils de Zigbee2MQTT.
    pub async fn rooms(&mut self) -> Result<(Vec<String>, BTreeMap<String, String>), String> {
        let areas = self
            .call(json!({ "type": "config/area_registry/list" }))
            .await?;
        let names: BTreeMap<String, String> = areas
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some((
                    a.get("area_id")?.as_str()?.to_string(),
                    a.get("name")?.as_str()?.trim().to_string(),
                ))
            })
            .collect();
        let devices = self
            .call(json!({ "type": "config/device_registry/list" }))
            .await?;
        let mut by_ieee = BTreeMap::new();
        for d in devices.as_array().into_iter().flatten() {
            let Some(area) = d
                .get("area_id")
                .and_then(Value::as_str)
                .and_then(|a| names.get(a))
            else {
                continue;
            };
            for ident in d
                .get("identifiers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let value = ident.get(1).and_then(Value::as_str).unwrap_or("");
                if let Some(ieee) = value.strip_prefix("zigbee2mqtt_") {
                    by_ieee.insert(ieee.to_lowercase(), area.clone());
                }
            }
        }
        let mut list: Vec<String> = names.into_values().filter(|n| !n.is_empty()).collect();
        list.sort();
        list.dedup();
        Ok((list, by_ieee))
    }

    /// Le module Zigbee2MQTT de Home Assistant OS (s'il existe) : (identifiant, nom, état).
    pub async fn z2m_addon(&mut self) -> Option<(String, String, String)> {
        let r = self
            .call(json!({ "type": "supervisor/api", "endpoint": "/addons", "method": "get" }))
            .await
            .ok()?;
        r.get("addons")?.as_array()?.iter().find_map(|a| {
            let slug = a.get("slug")?.as_str()?;
            slug.contains("zigbee2mqtt").then(|| {
                (
                    slug.to_string(),
                    a.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("Zigbee2MQTT")
                        .to_string(),
                    a.get("state")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                )
            })
        })
    }

    pub async fn stop_addon(&mut self, slug: &str) -> Result<(), String> {
        if !slug
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err("module invalide".into());
        }
        self.call(json!({ "type": "supervisor/api", "endpoint": format!("/addons/{slug}/stop"), "method": "post", "timeout": 60 }))
            .await
            .map(|_| ())
    }
}

/// `{"data":{"zip":"<base64>"},"status":"ok"}` → le zip.
pub fn backup_from_response(payload: &str) -> Result<Vec<u8>, String> {
    let v: Value = serde_json::from_str(payload)
        .map_err(|_| "réponse de Zigbee2MQTT illisible".to_string())?;
    if v.get("status").and_then(Value::as_str) != Some("ok") {
        let e = v.get("error").and_then(Value::as_str).unwrap_or("refusé");
        return Err(format!("Zigbee2MQTT refuse sa sauvegarde : {e}"));
    }
    let b64 = v
        .pointer("/data/zip")
        .and_then(Value::as_str)
        .ok_or("sauvegarde vide")?;
    let zip = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| "sauvegarde abîmée".to_string())?;
    if zip.len() > MAX_ZIP {
        return Err("sauvegarde trop grosse".into());
    }
    Ok(zip)
}

/// La même demande, directement sur un broker MQTT.
pub async fn z2m_backup_mqtt(
    env: &crate::plugins::MqttEnv,
    base_topic: &str,
) -> Result<Vec<u8>, String> {
    use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
    let mut o = MqttOptions::new(
        format!("heim-migrate-{}", std::process::id()),
        env.host.clone(),
        env.port,
    );
    o.set_keep_alive(Duration::from_secs(20));
    o.set_max_packet_size(MAX_ZIP * 2, 64 * 1024);
    if let Some(u) = &env.username {
        o.set_credentials(u.clone(), env.password.clone().unwrap_or_default());
    }
    let (c, mut ev) = AsyncClient::new(o, 16);
    let response = format!("{base_topic}/bridge/response/backup");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(45);
    loop {
        let e = tokio::time::timeout_at(deadline, ev.poll())
            .await
            .map_err(|_| {
                "Zigbee2MQTT n'a pas envoyé sa sauvegarde : est-il démarré ?".to_string()
            })?;
        match e {
            Ok(Event::Incoming(Packet::ConnAck(_))) => {
                c.try_subscribe(response.clone(), QoS::AtMostOnce)
                    .map_err(|e| e.to_string())?;
            }
            Ok(Event::Incoming(Packet::SubAck(_))) => {
                c.try_publish(
                    format!("{base_topic}/bridge/request/backup"),
                    QoS::AtLeastOnce,
                    false,
                    b"{}".to_vec(),
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(Event::Incoming(Packet::Publish(p))) if p.topic == response => {
                return backup_from_response(&String::from_utf8_lossy(&p.payload));
            }
            Ok(_) => {}
            Err(e) => return Err(format!("broker MQTT injoignable : {e}")),
        }
    }
}

/// Les fichiers du zip qu'on garde : ceux du dossier de Zigbee2MQTT, sans ses journaux.
pub fn unzip(zip: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(zip))
        .map_err(|e| format!("zip illisible : {e}"))?;
    let mut out = Vec::new();
    for i in 0..a.len() {
        let mut f = a.by_index(i).map_err(|e| e.to_string())?;
        if f.is_dir() {
            continue;
        }
        let Some(path) = f.enclosed_name() else {
            continue;
        };
        let name = path.to_string_lossy().replace('\\', "/");
        // Seulement le premier niveau (configuration, base, sauvegarde) ; pas les journaux.
        if name.contains('/') || name.starts_with('.') {
            continue;
        }
        if f.size() > MAX_ZIP as u64 {
            return Err(format!("{name} : trop gros"));
        }
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((name, buf));
    }
    if !out.iter().any(|(n, _)| n == "configuration.yaml") {
        return Err("pas de configuration.yaml dans la sauvegarde".into());
    }
    Ok(out)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingRooms {
    /// Adresse IEEE → pièce.
    pub rooms: BTreeMap<String, String>,
}

/// Ajoute les pièces de Home Assistant à la maison (sans toucher aux existantes) et retient
/// la pièce de chaque appareil Zigbee, posée dès que ses entités apparaissent.
pub fn remember_rooms(
    core: &crate::Core,
    names: &[String],
    by_ieee: BTreeMap<String, String>,
) -> Result<usize, String> {
    use crate::settings::{HOME, HomeLayout, RoomDef};
    let mut home: HomeLayout = core.settings.get(HOME).unwrap_or_default();
    let mut added = 0;
    for n in names {
        let n: String = n.trim().chars().take(40).collect();
        if n.is_empty() || home.rooms.len() >= 80 {
            continue;
        }
        if !home
            .rooms
            .iter()
            .any(|r| r.name.to_lowercase() == n.to_lowercase())
        {
            home.rooms.push(RoomDef {
                name: n,
                floor: None,
                icon: None,
            });
            added += 1;
        }
    }
    if added > 0 {
        home.validate()?;
        core.settings.set(HOME, &home).map_err(|e| e.to_string())?;
        core.bus
            .publish(crate::bus::Event::SettingsChanged { key: HOME.into() });
    }
    // Les noms exacts de la maison (casse comprise).
    let canon = |n: &str| {
        home.rooms
            .iter()
            .find(|r| r.name.to_lowercase() == n.trim().to_lowercase())
            .map(|r| r.name.clone())
    };
    let mut pending: PendingRooms = core.settings.get(ROOMS_KEY).unwrap_or_default();
    for (ieee, room) in by_ieee {
        if let Some(r) = canon(&room) {
            pending.rooms.insert(ieee, r);
        }
    }
    core.settings
        .set(ROOMS_KEY, &pending)
        .map_err(|e| e.to_string())?;
    core.bus.publish(crate::bus::Event::SettingsChanged {
        key: ROOMS_KEY.into(),
    });
    Ok(added)
}

async fn place(core: &crate::Core, e: &crate::model::EntityView, pending: &PendingRooms) {
    if e.meta.integration != "zigbee2mqtt" || e.meta.room.is_some() || e.meta.user_edited {
        return;
    }
    let Some(room) = e
        .meta
        .device_id
        .as_ref()
        .and_then(|d| pending.rooms.get(&d.to_lowercase()))
    else {
        return;
    };
    let patch = crate::registry::MetaPatch {
        room: Some(Some(room.clone())),
        ..Default::default()
    };
    let _ = core.registry.update_meta(&e.meta.entity_id, patch).await;
}

/// Pose les pièces retenues sur les entités Zigbee qui arrivent.
pub fn spawn_rooms(core: crate::Core) {
    let mut rx = core.bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(crate::bus::Event::EntityUpdated { entity }) => {
                    let pending: PendingRooms = core.settings.get(ROOMS_KEY).unwrap_or_default();
                    if !pending.rooms.is_empty() {
                        place(&core, &entity, &pending).await;
                    }
                }
                Ok(crate::bus::Event::SettingsChanged { key }) if key == ROOMS_KEY => {
                    let pending: PendingRooms = core.settings.get(ROOMS_KEY).unwrap_or_default();
                    for e in core.registry.snapshot().await {
                        place(&core, &e, &pending).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn urls() {
        assert_eq!(
            ws_url("homeassistant.local").unwrap(),
            "ws://homeassistant.local:8123/api/websocket"
        );
        assert_eq!(
            ws_url("http://192.168.1.10:8123/").unwrap(),
            "ws://192.168.1.10:8123/api/websocket"
        );
        assert!(ws_url("https://x.ui.nabu.casa").is_err());
        assert!(ws_url("").is_err());
    }

    #[test]
    fn backup_zip() {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("configuration.yaml", o).unwrap();
            z.write_all(b"mqtt:\n  server: mqtt://core-mosquitto\n")
                .unwrap();
            z.start_file("database.db", o).unwrap();
            z.write_all(b"{\"id\":1}\n").unwrap();
            z.start_file("log/2026/log.log", o).unwrap();
            z.write_all(b"x").unwrap();
            z.start_file("../evil", o).unwrap();
            z.write_all(b"x").unwrap();
            z.finish().unwrap();
        }
        let raw = buf.into_inner();
        let payload = json!({ "status": "ok", "data": { "zip": base64::engine::general_purpose::STANDARD.encode(&raw) } });
        let zip = backup_from_response(&payload.to_string()).unwrap();
        let files = unzip(&zip).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["configuration.yaml", "database.db"]);
        assert!(backup_from_response(r#"{"status":"error","error":"non"}"#).is_err());
    }
}
