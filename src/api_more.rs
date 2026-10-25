//! Routes de la v0.3 : réglages, système, sauvegarde, pièces, automatismes, « allumer pour ».

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppState, bad_request, not_found};
use crate::automations::AutomationInput;
use crate::settings::{self, AlertSettings, Location, Zigbee2MqttSettings};
use crate::store;

// ---- Allumer pour X minutes -------------------------------------------------------

#[derive(Deserialize)]
pub struct ForInput {
    minutes: f64,
}

pub async fn turn_on_for(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ForInput>,
) -> ApiResult<impl IntoResponse> {
    if !input.minutes.is_finite() || input.minutes <= 0.0 {
        return Err(bad_request("durée invalide"));
    }
    let seconds = (input.minutes * 60.0).round() as u64;
    app.core()
        .timers
        .run_for(&id, seconds)
        .await
        .map(Json)
        .map_err(bad_request)
}

// ---- Réglages ---------------------------------------------------------------------

/// Le mot de passe MQTT ne ressort jamais de Heim : on dit seulement s'il existe.
fn public_zigbee(value: Value) -> Value {
    let mut v = value;
    if let Some(obj) = v.as_object_mut() {
        let has = obj
            .get("password")
            .and_then(Value::as_str)
            .is_some_and(|p| !p.is_empty());
        obj.remove("password");
        obj.insert("has_password".into(), json!(has));
    }
    v
}

pub async fn get_setting(
    State(app): State<AppState>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    let core = app.core();
    let value = core.settings.get_raw(&key);
    let value = match key.as_str() {
        settings::ZIGBEE2MQTT => value.map(public_zigbee),
        settings::LOCATION => value.or_else(|| serde_json::to_value(Location::default()).ok()),
        settings::ALERTS => value.or_else(|| serde_json::to_value(AlertSettings::default()).ok()),
        settings::HOME => {
            value.or_else(|| serde_json::to_value(settings::HomeLayout::default()).ok())
        }
        // L'alarme a ses propres routes : le code haché ne sort pas d'ici.
        crate::security::CONFIG_KEY => Some(core.security.config().public()),
        k if k.starts_with("security.") || k.starts_with("push.") || k.starts_with("update.") => {
            None
        }
        // Clés locales Tuya : seulement par /api/tuya, sans les clés.
        "tuya" => None,
        _ => value,
    };
    Json(value.unwrap_or(Value::Null))
}

/// Clés que PUT /settings/{clé} ne doit jamais écrire directement.
pub fn reserved_key(key: &str) -> bool {
    key.starts_with("security")
        || key.starts_with("push.")
        || key.starts_with("update")
        || key.starts_with("setup.")
        || matches!(
            key,
            "tuya"
                | "matter"
                | "presence"
                | crate::groups::SETTINGS_KEY
                | crate::api_controls::POWER_ON_KEY
        )
}

pub async fn put_setting(
    State(app): State<AppState>,
    Path(key): Path<String>,
    Json(mut value): Json<Value>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    // Réglages internes (alarme, clés push, mises à jour, clés Tuya…) : seulement par leurs
    // propres routes, qui vérifient ce qu'on y met.
    if reserved_key(&key) {
        return Err(bad_request("ce réglage se modifie par sa propre page"));
    }
    match key.as_str() {
        settings::ZIGBEE2MQTT => {
            // Mot de passe absent = on garde l'actuel ; chaîne vide = on l'efface.
            if value.get("password").is_none_or(Value::is_null) {
                let current = core
                    .settings
                    .get::<Zigbee2MqttSettings>(settings::ZIGBEE2MQTT)
                    .and_then(|s| s.password);
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("password".into(), json!(current));
                }
            }
            if let Some(obj) = value.as_object_mut() {
                obj.remove("has_password");
            }
            let parsed: Zigbee2MqttSettings = serde_json::from_value(value.clone())
                .map_err(|e| bad_request(format!("réglage invalide : {e}")))?;
            if parsed.enabled && parsed.host.trim().is_empty() {
                return Err(bad_request(
                    "indiquez l'adresse du broker MQTT (l'IP de la machine Home Assistant ou Mosquitto)",
                ));
            }
            core.settings
                .set(settings::ZIGBEE2MQTT, &parsed)
                .map_err(bad_request)?;
            core.apply_zigbee_settings();
            Ok(Json(public_zigbee(
                serde_json::to_value(parsed).unwrap_or_default(),
            )))
        }
        settings::LOCATION => {
            let loc: Location = serde_json::from_value(value)
                .map_err(|e| bad_request(format!("lieu invalide : {e}")))?;
            if !(-90.0..=90.0).contains(&loc.latitude) || !(-180.0..=180.0).contains(&loc.longitude)
            {
                return Err(bad_request("latitude ou longitude hors limites"));
            }
            core.settings
                .set(settings::LOCATION, &loc)
                .map_err(bad_request)?;
            Ok(Json(json!(loc)))
        }
        settings::ALERTS => {
            let a: AlertSettings = serde_json::from_value(value)
                .map_err(|e| bad_request(format!("réglage invalide : {e}")))?;
            core.settings
                .set(settings::ALERTS, &a)
                .map_err(bad_request)?;
            Ok(Json(json!(a)))
        }
        settings::HOME => {
            let mut h: settings::HomeLayout = serde_json::from_value(value)
                .map_err(|e| bad_request(format!("maison invalide : {e}")))?;
            h.validate().map_err(bad_request)?;
            core.settings.set(settings::HOME, &h).map_err(bad_request)?;
            Ok(Json(json!(h)))
        }
        // Réglages de l'interface (tableau de bord, ordre des pièces...) : rangés tels quels.
        k if k.starts_with("ui.") => {
            core.settings
                .set_raw(k, value.clone())
                .map_err(bad_request)?;
            Ok(Json(value))
        }
        _ => Err(not_found(&key)),
    }
}

// ---- Système ----------------------------------------------------------------------

fn local_ip() -> Option<String> {
    // Aucun paquet n'est envoyé : on demande seulement au système quelle interface il utiliserait.
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn hostname() -> Option<String> {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
}

fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

pub async fn system(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let db = core.data_dir.join("heim.db");
    let db_size = std::fs::metadata(&db).map(|m| m.len()).ok();
    let entities = core.registry.snapshot().await.len();
    let now = chrono::Local::now();
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_s": core.started.elapsed().as_secs(),
        "memory_kib": rss_kib(),
        "hostname": hostname(),
        "ip": local_ip(),
        "port": app.port(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "data_dir": core.data_dir.display().to_string(),
        "supervised": crate::update::supervised(),
        "netsetup": crate::netsetup::enabled(),
        "db_bytes": db_size,
        "entities": entities,
        "local_time": now.to_rfc3339(),
    }))
}

// ---- Sauvegarde -------------------------------------------------------------------

pub async fn backup(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let tmp = core.data_dir.join("heim-backup.tmp");
    let store = core.store.clone();
    let path = tmp.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        store.backup_blocking(&path)?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string());
        let _ = std::fs::remove_file(&path);
        bytes
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| {
        ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("sauvegarde impossible : {e}"),
        )
    })?;
    let name = format!("heim-{}.db", chrono::Local::now().format("%Y-%m-%d"));
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    ))
}

pub async fn restore(State(app): State<AppState>, body: Bytes) -> ApiResult<impl IntoResponse> {
    if !store::looks_like_sqlite(&body) {
        return Err(bad_request("ce fichier n'est pas une sauvegarde Heim"));
    }
    let core = app.core();
    let target = store::restore_path(&core.data_dir.join("heim.db"));
    tokio::task::spawn_blocking(move || std::fs::write(&target, &body))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(
        json!({ "restart_required": true, "message": "Sauvegarde prête : redémarrez Heim pour la charger." }),
    ))
}

// ---- Pièces -----------------------------------------------------------------------

#[derive(Deserialize)]
pub struct RenameRoom {
    from: String,
    #[serde(default)]
    to: Option<String>,
}

pub async fn rename_room(
    State(app): State<AppState>,
    Json(input): Json<RenameRoom>,
) -> impl IntoResponse {
    let count = app.core().registry.rename_room(&input.from, input.to).await;
    Json(json!({ "updated": count }))
}

// ---- Automatismes -----------------------------------------------------------------

pub async fn list_automations(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core().automations.list().await)
}

pub async fn put_automation(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AutomationInput>,
) -> ApiResult<impl IntoResponse> {
    app.core()
        .automations
        .upsert(&id, input)
        .await
        .map(Json)
        .map_err(bad_request)
}

pub async fn delete_automation(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if app.core().automations.delete(&id).await {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&id))
    }
}

pub async fn run_automation(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    app.core().automations.run(&id).await.map_err(bad_request)?;
    Ok(StatusCode::ACCEPTED)
}

// ---- Supprimer un appareil disparu -------------------------------------------------

/// Retire une entité. Refusé si l'appareil répond encore : il réapparaîtrait aussitôt.
/// Sert aux appareils retirés du réseau Zigbee, ou aux restes d'essais.
pub async fn delete_entity(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let core = app.core();
    let entity = core.registry.get(&id).await.ok_or_else(|| not_found(&id))?;
    let live = entity.state.as_ref().is_some_and(|s| s.available)
        && !matches!(
            entity.meta.domain,
            crate::model::Domain::Scene | crate::model::Domain::Timer
        );
    if live {
        return Err(bad_request(
            "cet appareil répond encore : retirez-le d'abord de Zigbee2MQTT, ou masquez-le",
        ));
    }
    core.registry.remove(&id).await;
    core.timers.delete(&crate::timers::auto_timer_id(&id)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Mises à jour, redémarrage ---------------------------------------------------------

use crate::update::{self, UpdateSettings};

pub async fn get_update(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let s: UpdateSettings = core.settings.get(update::SETTINGS_KEY).unwrap_or_default();
    let mut status = core.updater.status();
    if status.repo.is_none() {
        status.repo = s.repo();
    }
    Json(json!({ "status": status, "settings": s, "default_repo": update::default_repo() }))
}

pub async fn check_update(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let s: UpdateSettings = core.settings.get(update::SETTINGS_KEY).unwrap_or_default();
    Json(core.updater.check(&s).await)
}

pub async fn put_update_settings(
    State(app): State<AppState>,
    Json(mut s): Json<UpdateSettings>,
) -> ApiResult<impl IntoResponse> {
    if let Some(r) = &s.repo {
        let r = r
            .trim()
            .trim_start_matches("https://github.com/")
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_string();
        if !r.is_empty() && (r.split('/').count() != 2 || r.contains(' ')) {
            return Err(bad_request(
                "dépôt attendu sous la forme proprietaire/nom (exemple : alex/heim)",
            ));
        }
        s.repo = Some(r).filter(|r| !r.is_empty());
    }
    app.core()
        .settings
        .set(update::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    Ok(Json(s))
}

pub async fn install_update(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let version = core
        .updater
        .install(core.store.clone())
        .await
        .map_err(bad_request)?;
    update::restart_soon(core.store.clone());
    Ok(Json(json!({ "installed": version, "restarting": true })))
}

pub async fn restart(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    if !update::supervised() {
        return Err(bad_request(
            "Heim ne tourne pas comme service : il ne redémarrerait pas tout seul",
        ));
    }
    update::restart_soon(app.core().store.clone());
    Ok(Json(json!({ "restarting": true })))
}

// ---- Intégrations : la même carte pour toutes ------------------------------------------

use crate::integrations::{matter, tuya, zigbee2mqtt};

pub async fn integrations_catalog(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let status = core
        .integrations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let entities = core.registry.snapshot().await;
    let count = |name: &str| {
        let mut devices: Vec<String> = entities
            .iter()
            .filter(|e| e.meta.integration == name)
            .map(|e| {
                e.meta
                    .device_id
                    .clone()
                    .unwrap_or_else(|| e.meta.entity_id.clone())
            })
            .collect();
        devices.sort();
        devices.dedup();
        devices.len()
    };
    let z: Option<Zigbee2MqttSettings> = core.settings.get(settings::ZIGBEE2MQTT);
    let t: tuya::TuyaSettings = core.settings.get(tuya::SETTINGS_KEY).unwrap_or_default();
    let m: matter::MatterSettings = core.settings.get(matter::SETTINGS_KEY).unwrap_or_default();
    let entry = |id: &str,
                 name: &str,
                 description: &str,
                 icon: &str,
                 enabled: bool,
                 configured: bool,
                 summary: String| {
        let st = status.get(id);
        json!({
            "id": id, "name": name, "description": description, "icon": icon,
            "enabled": enabled, "configured": configured,
            "status": st.and_then(|s| s.get("status").cloned()),
            "detail": st.and_then(|s| s.get("detail").cloned()),
            "devices": count(id),
            "summary": summary,
        })
    };
    Json(json!([
        entry(
            zigbee2mqtt::INTEGRATION,
            "Zigbee",
            "Par Zigbee2MQTT et son broker MQTT",
            "plug",
            z.as_ref().is_some_and(|s| s.enabled),
            z.as_ref().is_some_and(|s| !s.host.trim().is_empty()),
            z.as_ref()
                .map(|s| format!("{}:{}", s.host, s.port))
                .unwrap_or_default(),
        ),
        entry(
            tuya::INTEGRATION,
            "Tuya",
            "Prises, ampoules Wi-Fi en local (LSC, Lidl…)",
            "bolt",
            t.enabled,
            !t.devices.is_empty(),
            format!("{} appareil(s) déclaré(s)", t.devices.len()),
        ),
        entry(
            matter::INTEGRATION,
            "Matter",
            "Par le serveur Matter de Home Assistant",
            "grid",
            m.enabled,
            !m.url.trim().is_empty(),
            m.url.clone(),
        ),
    ]))
}

#[derive(Deserialize)]
pub struct EnabledInput {
    enabled: bool,
}

pub async fn set_integration_enabled(
    State(app): State<AppState>,
    Path(name): Path<String>,
    Json(input): Json<EnabledInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    match name.as_str() {
        zigbee2mqtt::INTEGRATION => {
            let mut s: Zigbee2MqttSettings = core
                .settings
                .get(settings::ZIGBEE2MQTT)
                .ok_or_else(|| bad_request("indiquez d'abord l'adresse du broker MQTT"))?;
            if input.enabled && s.host.trim().is_empty() {
                return Err(bad_request("indiquez d'abord l'adresse du broker MQTT"));
            }
            s.enabled = input.enabled;
            core.settings
                .set(settings::ZIGBEE2MQTT, &s)
                .map_err(bad_request)?;
            core.apply_zigbee_settings();
        }
        tuya::INTEGRATION => {
            let mut s: tuya::TuyaSettings =
                core.settings.get(tuya::SETTINGS_KEY).unwrap_or_default();
            s.enabled = input.enabled;
            core.settings
                .set(tuya::SETTINGS_KEY, &s)
                .map_err(bad_request)?;
            core.apply_tuya_settings();
        }
        matter::INTEGRATION => {
            let mut s: matter::MatterSettings =
                core.settings.get(matter::SETTINGS_KEY).unwrap_or_default();
            if input.enabled && s.url.trim().is_empty() {
                return Err(bad_request("indiquez d'abord l'adresse du serveur Matter"));
            }
            s.enabled = input.enabled;
            core.settings
                .set(matter::SETTINGS_KEY, &s)
                .map_err(bad_request)?;
            core.apply_matter_settings();
        }
        _ => return Err(not_found(&name)),
    }
    Ok(Json(json!({ "id": name, "enabled": input.enabled })))
}
