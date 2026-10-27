//! Routes de la v0.15 : le broker MQTT intégré et Zigbee2MQTT sur la box.

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::Core;
use crate::api::{ApiResult, AppState, bad_request, not_found};
use crate::broker::{self, BrokerSettings, BrokerUser};
use crate::settings::{self, Zigbee2MqttSettings};
use crate::zigbee_local::{self, LocalSettings, Runner};

fn broker_settings(core: &Core) -> BrokerSettings {
    core.settings.get(broker::SETTINGS_KEY).unwrap_or_default()
}

fn local_settings(core: &Core) -> LocalSettings {
    core.settings
        .get(zigbee_local::SETTINGS_KEY)
        .unwrap_or_default()
}

// ---- Broker -----------------------------------------------------------------------

pub async fn get_broker(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core().broker.status())
}

#[derive(Deserialize)]
pub struct BrokerInput {
    enabled: bool,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    open: bool,
}

pub async fn put_broker(
    State(app): State<AppState>,
    Json(input): Json<BrokerInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let local = local_settings(core);
    if !input.enabled && local.enabled {
        return Err(bad_request(
            "Zigbee2MQTT sur la box utilise le broker : arrêtez d'abord Zigbee sur la box",
        ));
    }
    let mut s = broker_settings(core);
    let old_port = s.port;
    s.enabled = input.enabled;
    s.open = input.open;
    if let Some(p) = input.port {
        s.port = p;
    }
    s.validate().map_err(bad_request)?;
    core.settings
        .set(broker::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_broker_settings().await.map_err(bad_request)?;
    if local.enabled && s.port != old_port {
        // Zigbee2MQTT et Heim doivent suivre le nouveau port.
        start_local(core, &local).await.map_err(bad_request)?;
    }
    Ok(Json(core.broker.status()))
}

#[derive(Deserialize)]
pub struct UserInput {
    username: String,
}

/// Un compte pour un appareil du réseau (Tasmota, Shelly…). Le mot de passe est montré une fois.
pub async fn add_broker_user(
    State(app): State<AppState>,
    Json(input): Json<UserInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let mut s = broker_settings(core);
    let username = input.username.trim().to_string();
    if s.users.iter().any(|u| u.username == username) {
        return Err(bad_request(format!(
            "le compte MQTT « {username} » existe déjà"
        )));
    }
    let password = broker::generate_password();
    s.users.push(BrokerUser {
        username: username.clone(),
        password: password.clone(),
    });
    s.validate().map_err(bad_request)?;
    core.settings
        .set(broker::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_broker_settings().await.map_err(bad_request)?;
    Ok(Json(json!({ "username": username, "password": password })))
}

pub async fn delete_broker_user(
    State(app): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let mut s = broker_settings(core);
    let before = s.users.len();
    s.users.retain(|u| u.username != name);
    if s.users.len() == before {
        return Err(not_found(&name));
    }
    core.settings
        .set(broker::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_broker_settings().await.map_err(bad_request)?;
    Ok(Json(core.broker.status()))
}

// ---- Zigbee2MQTT sur la box ----------------------------------------------------------

pub async fn local_status(core: &Core) -> Value {
    let z = &core.zigbee_local;
    let s = local_settings(core);
    let state = z.state().await;
    let dongles = z.dongles();
    let broker = broker_settings(core);
    json!({
        "installed": z.installed(),
        "runner": match &z.runner { Runner::Systemd => "systemd", Runner::Child(_) => "child", Runner::Missing => "missing" },
        "version": z.version(),
        "state": state,
        "enabled": s.enabled,
        "port": s.port,
        "adapter": s.adapter,
        "frontend": s.frontend,
        "frontend_port": zigbee_local::FRONTEND_PORT,
        "frontend_token": if s.frontend { s.frontend_token.clone() } else { None },
        "dongles": dongles,
        "config": z.config_summary(),
        "broker": { "enabled": broker.enabled, "port": broker.port, "running": core.broker.running() },
        "bridge": bridge_online(core),
        "log": z.log_tail(30),
    })
}

/// Zigbee2MQTT annonce « online » (message retenu) sur `<topic>/bridge/state`.
fn bridge_online(core: &Core) -> bool {
    let topic = core
        .settings
        .get::<Zigbee2MqttSettings>(settings::ZIGBEE2MQTT)
        .map(|z| z.base_topic)
        .unwrap_or_else(|| "zigbee2mqtt".into());
    core.broker
        .retained(&format!("{topic}/bridge/state"))
        .is_some_and(|p| String::from_utf8_lossy(&p).contains("online"))
}

pub async fn get_local(State(app): State<AppState>) -> impl IntoResponse {
    Json(local_status(app.core()).await)
}

#[derive(Deserialize)]
pub struct LocalInput {
    enabled: bool,
    #[serde(default)]
    port: Option<String>,
    #[serde(default)]
    adapter: Option<String>,
    #[serde(default)]
    frontend: bool,
}

/// Démarre Zigbee2MQTT sur la box : broker allumé, configuration écrite, Heim branché dessus.
pub async fn start_local(core: &Core, s: &LocalSettings) -> Result<(), String> {
    if !core.zigbee_local.installed() {
        return Err("Zigbee2MQTT n'est pas installé sur cette box".into());
    }
    let mut b = broker_settings(core);
    if !b.enabled {
        b.enabled = true;
        core.settings
            .set(broker::SETTINGS_KEY, &b)
            .map_err(|e| e.to_string())?;
    }
    core.apply_broker_settings().await?;
    core.zigbee_local.write_config(s, b.port)?;
    // Heim lit Zigbee2MQTT sur son propre broker.
    let previous: Option<Zigbee2MqttSettings> = core.settings.get(settings::ZIGBEE2MQTT);
    let topic = core
        .zigbee_local
        .config_summary()
        .get("base_topic")
        .and_then(Value::as_str)
        .map(str::to_string);
    let wanted = Zigbee2MqttSettings {
        enabled: true,
        host: "127.0.0.1".into(),
        port: b.port,
        username: None,
        password: None,
        base_topic: topic
            .or_else(|| previous.as_ref().map(|p| p.base_topic.clone()))
            .unwrap_or_else(|| "zigbee2mqtt".into()),
    };
    if previous.as_ref() != Some(&wanted) {
        core.settings
            .set(settings::ZIGBEE2MQTT, &wanted)
            .map_err(|e| e.to_string())?;
        core.apply_zigbee_settings();
    }
    core.zigbee_local.start().await
}

pub async fn put_local(
    State(app): State<AppState>,
    Json(input): Json<LocalInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let mut s = local_settings(core);
    s.enabled = input.enabled;
    s.port = input.port.filter(|p| !p.trim().is_empty());
    s.adapter = input.adapter.filter(|a| !a.trim().is_empty());
    s.frontend = input.frontend;
    if s.frontend && s.frontend_token.is_none() {
        s.frontend_token = Some(zigbee_local::generate_token());
    }
    if s.enabled {
        if !core.zigbee_local.installed() {
            return Err(bad_request("Zigbee2MQTT n'est pas installé sur cette box"));
        }
        if s.port.is_none() && core.zigbee_local.dongles().is_empty() {
            return Err(bad_request("aucune clé Zigbee branchée"));
        }
    }
    core.settings
        .set(zigbee_local::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    if s.enabled {
        start_local(core, &s).await.map_err(bad_request)?;
    } else {
        core.zigbee_local.stop().await.map_err(bad_request)?;
    }
    Ok(Json(local_status(core).await))
}

pub async fn restart_local(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let s = local_settings(core);
    if !s.enabled {
        return Err(bad_request("Zigbee2MQTT n'est pas activé sur la box"));
    }
    start_local(core, &s).await.map_err(bad_request)?;
    Ok(Json(local_status(core).await))
}

#[derive(Deserialize)]
pub struct ImportInput {
    configuration: String,
    /// Le contenu de database.db (du texte, une ligne JSON par appareil).
    #[serde(default)]
    database: Option<String>,
    #[serde(default)]
    coordinator_backup: Option<String>,
}

/// Reprend un Zigbee2MQTT existant (Home Assistant : dossier `zigbee2mqtt` de la configuration).
pub async fn import_local(
    State(app): State<AppState>,
    Json(input): Json<ImportInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let s = local_settings(core);
    let was_running = s.enabled;
    if was_running {
        let _ = core.zigbee_local.stop().await;
    }
    let summary = core
        .zigbee_local
        .import(
            &input.configuration,
            input.database.as_deref().map(str::as_bytes),
            input.coordinator_backup.as_deref(),
        )
        .map_err(bad_request)?;
    // La clé d'avant n'est plus forcément au même endroit : on garde le pilote, pas le port.
    let mut s = s;
    if s.adapter.is_none() {
        s.adapter = summary
            .get("adapter")
            .and_then(Value::as_str)
            .map(str::to_string);
        let _ = core.settings.set(zigbee_local::SETTINGS_KEY, &s);
    }
    if was_running {
        start_local(core, &s).await.map_err(bad_request)?;
    }
    core.bus.publish(crate::bus::Event::Log {
        kind: "system".into(),
        message: format!(
            "Zigbee2MQTT repris d'une autre installation ({} appareil(s))",
            summary["devices"].as_u64().unwrap_or(0)
        ),
        entity_id: None,
        actor: None,
    });
    Ok(Json(
        json!({ "imported": summary, "status": local_status(core).await }),
    ))
}

/// Au démarrage de Heim : Zigbee2MQTT local, s'il est activé, avec une configuration à jour.
pub fn spawn_at_start(core: Core) {
    let s = local_settings(&core);
    if !s.enabled || !core.zigbee_local.installed() {
        return;
    }
    tokio::spawn(async move {
        // Le broker d'abord (il démarre dans sa propre tâche).
        for _ in 0..50 {
            if core.broker.running() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let b = broker_settings(&core);
        if let Err(e) = core.zigbee_local.write_config(&s, b.port) {
            tracing::warn!("zigbee2mqtt local : {e}");
            return;
        }
        let state = core.zigbee_local.state().await;
        if state != "active"
            && state != "activating"
            && let Err(e) = core.zigbee_local.start().await
        {
            tracing::warn!("zigbee2mqtt local : {e}");
        }
    });
}
