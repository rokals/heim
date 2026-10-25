//! Routes de la v0.6 : Tuya et Matter.

use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiResult, AppState, bad_request};
use crate::integrations::{matter, tuya};

// ---- Tuya -------------------------------------------------------------------------

/// Les clés locales ne ressortent jamais : on dit seulement si elles sont là.
fn public_tuya(s: &tuya::TuyaSettings) -> Value {
    let devices: Vec<Value> = s
        .devices
        .iter()
        .map(|d| {
            let mapped = tuya::map_device(d);
            json!({
                "id": d.id,
                "name": d.name,
                "ip": d.ip,
                "version": d.version,
                "category": d.category,
                "product_name": d.product_name,
                "has_key": d.key.len() == 16,
                "supported": !mapped.is_empty(),
                "entities": mapped.iter().map(|m| m.meta.entity_id.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "enabled": s.enabled, "devices": devices })
}

pub async fn get_tuya(State(app): State<AppState>) -> impl IntoResponse {
    let s: tuya::TuyaSettings = app
        .core()
        .settings
        .get(tuya::SETTINGS_KEY)
        .unwrap_or_default();
    Json(public_tuya(&s))
}

#[derive(Deserialize)]
pub struct DeviceInput {
    id: String,
    #[serde(default)]
    name: String,
    /// Vide ou absent : on garde la clé actuelle.
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    ip: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
pub struct TuyaInput {
    enabled: bool,
    devices: Vec<DeviceInput>,
}

pub async fn put_tuya(
    State(app): State<AppState>,
    Json(input): Json<TuyaInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let current: tuya::TuyaSettings = core.settings.get(tuya::SETTINGS_KEY).unwrap_or_default();
    let mut devices = Vec::new();
    for d in input.devices {
        let id = d.id.trim().to_string();
        if id.is_empty() {
            return Err(bad_request("chaque appareil doit avoir un identifiant"));
        }
        let old = current.devices.iter().find(|o| o.id == id);
        let key = match d
            .key
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
        {
            Some(k) => k,
            None => old.map(|o| o.key.clone()).unwrap_or_default(),
        };
        if key.len() != 16 {
            return Err(bad_request(format!(
                "{} : la clé locale doit faire 16 caractères",
                if d.name.is_empty() { &id } else { &d.name }
            )));
        }
        let ip = d.ip.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(ip) = &ip
            && ip.parse::<std::net::Ipv4Addr>().is_err()
        {
            return Err(bad_request(format!("adresse IP « {ip} » invalide")));
        }
        let version = d.version.filter(|v| !v.is_empty());
        if let Some(v) = &version
            && crate::integrations::tuya_proto::Version::parse(v).is_none()
        {
            return Err(bad_request(format!(
                "version « {v} » inconnue (3.3, 3.4 ou 3.5)"
            )));
        }
        devices.push(tuya::TuyaDevice {
            name: if d.name.trim().is_empty() {
                old.map(|o| o.name.clone()).unwrap_or_else(|| id.clone())
            } else {
                d.name.trim().to_string()
            },
            key,
            ip,
            version,
            category: d
                .category
                .filter(|c| !c.is_empty())
                .or_else(|| old.and_then(|o| o.category.clone())),
            product_name: old.and_then(|o| o.product_name.clone()),
            mapping: old.map(|o| o.mapping.clone()).unwrap_or_default(),
            id,
        });
    }
    let s = tuya::TuyaSettings {
        enabled: input.enabled,
        devices,
    };
    core.settings
        .set(tuya::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_tuya_settings();
    Ok(Json(public_tuya(&s)))
}

/// Ajoute (ou met à jour) les appareils d'un `devices.json` de tinytuya.
pub async fn import_tuya(
    State(app): State<AppState>,
    body: String,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let imported = tuya::import_tinytuya(&body).map_err(bad_request)?;
    let mut s: tuya::TuyaSettings = core.settings.get(tuya::SETTINGS_KEY).unwrap_or_default();
    let mut added = 0;
    for d in imported {
        match s.devices.iter_mut().find(|o| o.id == d.id) {
            Some(o) => {
                o.key = d.key;
                o.mapping = d.mapping;
                o.category = d.category.or(o.category.take());
                o.product_name = d.product_name.or(o.product_name.take());
                if d.ip.is_some() {
                    o.ip = d.ip;
                }
                if d.version.is_some() {
                    o.version = d.version;
                }
            }
            None => {
                s.devices.push(d);
                added += 1;
            }
        }
    }
    s.enabled = true;
    core.settings
        .set(tuya::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_tuya_settings();
    let mut out = public_tuya(&s);
    out["added"] = json!(added);
    Ok(Json(out))
}

/// Les appareils Tuya qui s'annoncent sur le réseau (adresse IP, version).
pub async fn scan_tuya(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let found: Vec<tuya::Announce> = if core.tuya_running() {
        tokio::time::sleep(Duration::from_secs(6)).await;
        core.tuya_seen
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    } else {
        tuya::scan(6).await
    };
    Json(found)
}

// ---- Matter -----------------------------------------------------------------------

fn matter_url(app: &AppState) -> Option<String> {
    let s: matter::MatterSettings = app
        .core()
        .settings
        .get(matter::SETTINGS_KEY)
        .unwrap_or_default();
    (!s.url.trim().is_empty()).then(|| matter::normalize_url(&s.url))
}

pub async fn get_matter(State(app): State<AppState>) -> impl IntoResponse {
    let s: matter::MatterSettings = app
        .core()
        .settings
        .get(matter::SETTINGS_KEY)
        .unwrap_or_default();
    let mut out = json!({ "enabled": s.enabled, "url": s.url });
    if let Some(url) = matter_url(&app).filter(|_| s.enabled) {
        match matter::server_info(&url).await {
            Ok(info) => {
                out["server"] = info;
                if let Ok(nodes) =
                    matter::request(&url, "get_nodes", json!({}), Duration::from_secs(10)).await
                {
                    out["nodes"] = json!(matter::summarize(
                        nodes.as_array().map(Vec::as_slice).unwrap_or(&[])
                    ));
                }
            }
            Err(e) => out["error"] = json!(e),
        }
    }
    Json(out)
}

pub async fn put_matter(
    State(app): State<AppState>,
    Json(s): Json<matter::MatterSettings>,
) -> ApiResult<impl IntoResponse> {
    if s.enabled && s.url.trim().is_empty() {
        return Err(bad_request("indiquez l'adresse du serveur Matter"));
    }
    let url = matter::normalize_url(&s.url);
    if s.enabled {
        matter::server_info(&url).await.map_err(bad_request)?;
    }
    let core = app.core();
    core.settings
        .set(matter::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_matter_settings();
    Ok(Json(
        json!({ "enabled": s.enabled, "url": s.url, "normalized": url }),
    ))
}

#[derive(Deserialize)]
pub struct CommissionInput {
    code: String,
}

/// Appairage par code (« MT:… » ou les 11 chiffres), pour un appareil déjà sur le réseau :
/// par exemple partagé depuis Apple Maison, Google Home ou Home Assistant.
pub async fn commission(
    State(app): State<AppState>,
    Json(input): Json<CommissionInput>,
) -> ApiResult<impl IntoResponse> {
    let url = matter_url(&app).ok_or_else(|| bad_request("serveur Matter non configuré"))?;
    let code: String = input
        .code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    if code.is_empty() {
        return Err(bad_request("code d'appairage vide"));
    }
    let node = matter::request(
        &url,
        "commission_with_code",
        json!({ "code": code, "network_only": true }),
        Duration::from_secs(180),
    )
    .await
    .map_err(|e| bad_request(format!("appairage impossible : {e}")))?;
    Ok(Json(
        json!({ "node": matter::summarize(std::slice::from_ref(&node)) }),
    ))
}

#[derive(Deserialize)]
pub struct RemoveInput {
    node_id: u64,
}

pub async fn remove_node(
    State(app): State<AppState>,
    Json(input): Json<RemoveInput>,
) -> ApiResult<impl IntoResponse> {
    let url = matter_url(&app).ok_or_else(|| bad_request("serveur Matter non configuré"))?;
    matter::request(
        &url,
        "remove_node",
        json!({ "node_id": input.node_id }),
        Duration::from_secs(60),
    )
    .await
    .map_err(bad_request)?;
    Ok(Json(json!({ "removed": input.node_id })))
}
