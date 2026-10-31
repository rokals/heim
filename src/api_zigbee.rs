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
    // La découverte MQTT et les plugins suivent le broker.
    core.apply_plugins();
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
    // Zigbee2MQTT qui plante au démarrage n'écrit rien dans son fichier : le journal système
    // (sortie console) a alors le vrai message.
    let mut log = z.journal_tail(60).await;
    if log.is_empty() {
        log = z.log_tail(60);
    }
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
        "log": log,
        "hint": zigbee_local::explain(&log, &dongles, state == "failed" && zigbee_local::modem_manager_active().await),
        "error": if state == "failed" { zigbee_local::error_line(&log) } else { None },
        "network_known": z.network_known(),
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
    if !core.zigbee_local.installed() {
        return;
    }
    spawn_hotplug(core.clone());
    let s = local_settings(&core);
    if !s.enabled {
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

/// Clé branchée après coup : Zigbee2MQTT (activé, mais arrêté faute de clé) repart tout seul,
/// avec la configuration réécrite sur le chemin de la clé qui vient d'apparaître.
fn spawn_hotplug(core: Core) {
    tokio::spawn(async move {
        let paths = |core: &Core| -> Vec<String> {
            core.zigbee_local
                .dongles()
                .into_iter()
                .map(|d| d.path)
                .collect()
        };
        let mut known = paths(&core);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            let now = paths(&core);
            let arrived = now.iter().any(|p| !known.contains(p));
            known = now;
            if !arrived {
                continue;
            }
            let s = local_settings(&core);
            // Jamais de démarrage tout seul sur un réseau inconnu : la clé qu'on vient de
            // brancher porte peut-être celui de Home Assistant, pas encore repris.
            if !s.enabled || !core.zigbee_local.network_known() {
                continue;
            }
            let state = core.zigbee_local.state().await;
            if state == "active" && bridge_online(&core) {
                continue;
            }
            tracing::info!("zigbee2mqtt local : clé branchée, redémarrage");
            // Laisser le système finir de préparer le port série.
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            if let Err(e) = start_local(&core, &s).await {
                tracing::warn!("zigbee2mqtt local : {e}");
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Reprise d'une maison Home Assistant (voir `migrate`).

#[derive(Deserialize)]
pub struct HaInput {
    pub url: String,
    pub token: String,
    #[serde(default)]
    pub base_topic: Option<String>,
    #[serde(default)]
    pub slug: Option<String>,
}

fn topic_of(t: &Option<String>) -> Result<String, String> {
    let t = t
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("zigbee2mqtt");
    if t.contains(['#', '+']) || t.starts_with('/') || t.len() > 80 {
        return Err("sujet de base invalide".into());
    }
    Ok(t.to_string())
}

/// Vérifie l'accès à Home Assistant et cherche son module Zigbee2MQTT.
pub async fn ha_check(Json(input): Json<HaInput>) -> ApiResult<impl IntoResponse> {
    let mut ha = crate::migrate::Ha::connect(&input.url, &input.token)
        .await
        .map_err(bad_request)?;
    let addon = ha.z2m_addon().await;
    let (rooms, by_ieee) = ha.rooms().await.unwrap_or_default();
    Ok(Json(json!({
        "version": ha.version,
        "addon": addon.map(|(slug, name, state)| json!({ "slug": slug, "name": name, "state": state })),
        "rooms": rooms,
        "zigbee_devices_with_room": by_ieee.len(),
    })))
}

/// Les fichiers d'une sauvegarde de Zigbee2MQTT, posés pour le Zigbee2MQTT de la box.
async fn import_zip(core: &Core, zip: &[u8], origin: &str) -> Result<Value, String> {
    let files = crate::migrate::unzip(zip)?;
    let get = |n: &str| {
        files
            .iter()
            .find(|(f, _)| f == n)
            .map(|(_, d)| d.as_slice())
    };
    let configuration =
        String::from_utf8_lossy(get("configuration.yaml").unwrap_or_default()).to_string();
    let backup = get("coordinator_backup.json").map(|b| String::from_utf8_lossy(b).to_string());
    let s = local_settings(core);
    let running = s.enabled && core.zigbee_local.installed();
    if running {
        let _ = core.zigbee_local.stop().await;
    }
    let summary =
        core.zigbee_local
            .import(&configuration, get("database.db"), backup.as_deref())?;
    core.zigbee_local.import_extra(&files)?;
    let mut s = s;
    if s.adapter.is_none() {
        s.adapter = summary
            .get("adapter")
            .and_then(Value::as_str)
            .map(str::to_string);
        let _ = core.settings.set(zigbee_local::SETTINGS_KEY, &s);
    }
    if running {
        start_local(core, &s).await?;
    }
    core.bus.publish(crate::bus::Event::Log {
        kind: "system".into(),
        message: format!(
            "Zigbee2MQTT repris de {origin} ({} appareil(s))",
            summary["devices"].as_u64().unwrap_or(0)
        ),
        entity_id: None,
        actor: None,
    });
    Ok(summary)
}

/// Reprend Zigbee2MQTT et les pièces depuis Home Assistant.
pub async fn ha_import(
    State(app): State<AppState>,
    Json(input): Json<HaInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let topic = topic_of(&input.base_topic).map_err(bad_request)?;
    let mut ha = crate::migrate::Ha::connect(&input.url, &input.token)
        .await
        .map_err(bad_request)?;
    // Une seule clé : le module de HA a pu être arrêté. On le relance le temps de la sauvegarde
    // (il lui faut la clé : elle doit encore être branchée sur la machine Home Assistant).
    let mut addon = ha.z2m_addon().await;
    let mut started = false;
    if let Some((slug, _, state)) = &addon
        && state != "started"
    {
        ha.start_addon(slug).await.map_err(bad_request)?;
        started = true;
    }
    let wait = std::time::Duration::from_secs(if started { 120 } else { 45 });
    let zip = match ha.z2m_backup(&topic, wait).await {
        Ok(z) => z,
        Err(_) if addon.is_some() => {
            let state = ha.z2m_addon().await.map(|a| a.2).unwrap_or_default();
            return Err(bad_request(if state == "started" {
                "Zigbee2MQTT tourne dans Home Assistant mais ne répond pas : la clé Zigbee est-elle bien branchée sur la machine Home Assistant ? Avec une seule clé, laissez-la sur Home Assistant le temps de la reprise, puis déplacez-la sur la box."
            } else {
                "Zigbee2MQTT ne démarre pas dans Home Assistant : la clé Zigbee est-elle branchée sur la machine Home Assistant ? Avec une seule clé, rebranchez-la sur Home Assistant le temps de la reprise (une minute), puis déplacez-la sur la box."
            }));
        }
        Err(e) => return Err(bad_request(e)),
    };
    let (rooms, by_ieee) = ha.rooms().await.unwrap_or_default();
    if started {
        addon = ha.z2m_addon().await;
    }
    drop(ha);
    let summary = import_zip(core, &zip, "Home Assistant")
        .await
        .map_err(bad_request)?;
    let placed = by_ieee.len();
    let added = crate::migrate::remember_rooms(core, &rooms, by_ieee).map_err(bad_request)?;
    Ok(Json(json!({
        "imported": summary,
        "rooms_added": added,
        "devices_with_room": placed,
        "addon": addon.map(|(slug, name, state)| json!({ "slug": slug, "name": name, "state": state })),
        "status": local_status(core).await,
    })))
}

/// Arrête le module Zigbee2MQTT de Home Assistant (la clé ne doit servir qu'à un seul).
pub async fn ha_stop_addon(Json(input): Json<HaInput>) -> ApiResult<impl IntoResponse> {
    let slug = input
        .slug
        .clone()
        .ok_or_else(|| bad_request("module manquant"))?;
    let mut ha = crate::migrate::Ha::connect(&input.url, &input.token)
        .await
        .map_err(bad_request)?;
    ha.stop_addon(&slug).await.map_err(bad_request)?;
    Ok(Json(json!({ "stopped": slug })))
}

#[derive(Deserialize)]
pub struct MqttImportInput {
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub base_topic: Option<String>,
}

/// Même reprise, par le broker MQTT de l'autre installation.
pub async fn mqtt_import(
    State(app): State<AppState>,
    Json(input): Json<MqttImportInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let topic = topic_of(&input.base_topic).map_err(bad_request)?;
    let host = input.host.trim().to_string();
    if host.is_empty() || host.contains(['/', ' ']) {
        return Err(bad_request("adresse du broker invalide"));
    }
    let env = crate::plugins::MqttEnv {
        host,
        port: input.port.unwrap_or(1883),
        username: input.username.filter(|u| !u.is_empty()),
        password: input.password.filter(|p| !p.is_empty()),
    };
    let zip = crate::migrate::z2m_backup_mqtt(&env, &topic)
        .await
        .map_err(bad_request)?;
    let summary = import_zip(core, &zip, "un broker MQTT")
        .await
        .map_err(bad_request)?;
    Ok(Json(
        json!({ "imported": summary, "status": local_status(core).await }),
    ))
}
