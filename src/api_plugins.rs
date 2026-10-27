//! Routes de la v0.17 : les plugins (store, installation, réglages, journal).

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::Core;
use crate::api::{ApiResult, AppState, bad_request, not_found};
use crate::plugins::{self, Installed, Manifest, PluginsSettings};
use crate::settings::{self, Zigbee2MqttSettings};

fn lang(headers: &axum::http::HeaderMap) -> String {
    crate::api::lang_of(headers).to_string()
}

fn store_url(core: &Core, s: &PluginsSettings) -> Option<String> {
    s.store_url.clone().or_else(|| {
        let up: crate::update::UpdateSettings = core
            .settings
            .get(crate::update::SETTINGS_KEY)
            .unwrap_or_default();
        up.repo()
            .map(|r| format!("https://raw.githubusercontent.com/{r}/main/plugins/store.json"))
    })
}

/// Un plugin tel que l'interface le montre (store ou installé).
fn view(m: &Manifest, lang: &str) -> Value {
    json!({
        "id": m.id,
        "name": m.display_name(lang),
        "category": m.category,
        "author": m.author,
        "description": m.description.get(lang),
        "icon": m.icon.clone().unwrap_or_else(|| "grid".into()),
        "memory_mb": m.memory_mb,
        "memory_note": m.memory_note.as_ref().map(|n| n.get(lang)),
        "version": m.version,
        "builtin": m.builtin,
        "homepage": m.homepage,
        "permissions": m.permissions,
        "available": m.builtin || plugins::arch_key().is_some_and(|k| m.binaries.contains_key(k)) || m.manifest_url.is_some(),
    })
}

/// Mémoire et processeur d'un plugin externe, d'après le dernier relevé de la box.
fn usage(core: &Core, id: &str) -> Value {
    let snap = core.perf.last();
    let name = format!("plugin:{id}");
    snap.processes
        .iter()
        .find(|p| p.name == name)
        .map(|p| json!({ "memory": p.memory, "cpu": p.cpu }))
        .unwrap_or(Value::Null)
}

fn installed_view(core: &Core, p: &Installed, store: &[Manifest], lang: &str) -> Value {
    let m = if p.builtin {
        store.iter().find(|m| m.id == p.id).cloned()
    } else {
        p.manifest.clone()
    };
    let mut v = m
        .as_ref()
        .map(|m| view(m, lang))
        .unwrap_or_else(|| json!({ "id": p.id, "name": p.id }));
    v["installed"] = json!(true);
    v["enabled"] = json!(p.enabled);
    v["official"] = json!(p.official);
    v["source"] = json!(p.source);
    if !p.builtin {
        let defs = m.as_ref().map(|m| m.settings.clone()).unwrap_or_default();
        v["settings_schema"] = json!(
            defs.iter()
                .map(|d| json!({
                    "key": d.key, "type": d.kind, "label": d.label.get(lang),
                    "help": d.help.as_ref().map(|h| h.get(lang)), "required": d.required,
                    "default": d.default, "options": d.options,
                }))
                .collect::<Vec<_>>()
        );
        v["settings"] = json!(plugins::public_settings(&defs, &p.settings));
        v["run"] = json!(core.plugins.state(&p.id));
        v["usage"] = usage(core, &p.id);
    }
    v
}

pub async fn list(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let core = app.core();
    let lang = lang(&headers);
    let s = core.plugin_settings();
    let store = core.plugins.store(store_url(core, &s).as_deref()).await;
    let installed: Vec<Value> = s
        .installed
        .values()
        .map(|p| installed_view(core, p, &store, &lang))
        .collect();
    let catalog: Vec<Value> = store
        .iter()
        .map(|m| {
            let mut v = view(m, &lang);
            v["installed"] = json!(s.has(&m.id));
            v["official"] = json!(true);
            v
        })
        .collect();
    Json(json!({
        "installed": installed,
        "store": catalog,
        "categories": plugins::CATEGORIES,
        "arch": plugins::arch_key(),
    }))
}

#[derive(Deserialize)]
pub struct SourceInput {
    source: String,
}

/// Lit le manifeste d'un dépôt externe, sans rien installer (pour l'avertissement).
pub async fn preview(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<SourceInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let (m, url) = core
        .plugins
        .fetch_manifest(&input.source)
        .await
        .map_err(bad_request)?;
    let mut v = view(&m, &lang(&headers));
    v["source"] = json!(url);
    v["official"] = json!(false);
    v["installed"] = json!(core.plugin_settings().has(&m.id));
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct InstallInput {
    /// Un plugin du store (intégré ou officiel).
    #[serde(default)]
    id: Option<String>,
    /// Ou un dépôt externe.
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    settings: Map<String, Value>,
}

pub async fn install(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<InstallInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let lang = lang(&headers);
    let mut s = core.plugin_settings();
    let store = core.plugins.store(store_url(core, &s).as_deref()).await;
    let (manifest, source, official) = match (&input.id, &input.source) {
        (Some(id), _) => {
            let m = store
                .iter()
                .find(|m| &m.id == id)
                .cloned()
                .ok_or_else(|| not_found(id))?;
            if m.builtin {
                (m, None, true)
            } else {
                let url = m
                    .manifest_url
                    .clone()
                    .ok_or_else(|| bad_request("plugin du store sans manifeste"))?;
                let (full, url) = core
                    .plugins
                    .fetch_manifest(&url)
                    .await
                    .map_err(bad_request)?;
                (full, Some(url), true)
            }
        }
        (None, Some(src)) => {
            let (m, url) = core
                .plugins
                .fetch_manifest(src)
                .await
                .map_err(bad_request)?;
            (m, Some(url), false)
        }
        (None, None) => return Err(bad_request("indiquez un plugin ou un dépôt")),
    };
    if s.has(&manifest.id) && manifest.builtin {
        return Err(bad_request("déjà installé"));
    }
    if let Some(other) = s.installed.get(&manifest.id)
        && other.source != source
    {
        return Err(bad_request(format!(
            "un autre plugin « {} » est déjà installé",
            manifest.id
        )));
    }
    let mut entry = Installed {
        id: manifest.id.clone(),
        builtin: manifest.builtin,
        source: source.clone(),
        official,
        enabled: true,
        installed_at: Some(chrono::Utc::now()),
        ..Default::default()
    };
    if manifest.builtin {
        enable_builtin(core, &manifest.id, true);
    } else {
        entry.settings = plugins::clean_settings(&manifest.settings, &input.settings)
            .or_else(|e| {
                // Réglages obligatoires pas encore saisis : le plugin s'installe éteint.
                entry.enabled = false;
                plugins::clean_settings(
                    &manifest
                        .settings
                        .iter()
                        .cloned()
                        .map(|mut d| {
                            d.required = false;
                            d
                        })
                        .collect::<Vec<_>>(),
                    &input.settings,
                )
                .map_err(|_| e)
            })
            .map_err(bad_request)?;
        core.plugins
            .download(&manifest)
            .await
            .map_err(bad_request)?;
        entry.manifest = Some(manifest.clone());
        // Un plugin externe parle par la découverte MQTT.
        if !s.has("mqtt_discovery") {
            s.installed.insert(
                "mqtt_discovery".into(),
                Installed {
                    id: "mqtt_discovery".into(),
                    builtin: true,
                    official: true,
                    enabled: true,
                    installed_at: Some(chrono::Utc::now()),
                    ..Default::default()
                },
            );
        }
    }
    s.installed.insert(manifest.id.clone(), entry.clone());
    core.settings
        .set(plugins::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    core.apply_discovery();
    if !entry.builtin {
        core.apply_external(&entry);
    }
    core.bus.publish(crate::bus::Event::Log {
        kind: "system".into(),
        message: format!(
            "Plugin {} installé{}",
            manifest.display_name(&lang),
            if official { "" } else { " (dépôt externe)" }
        ),
        entity_id: None,
        actor: None,
    });
    Ok(Json(installed_view(core, &entry, &store, &lang)))
}

/// Allume ou éteint une connexion intégrée (comme son interrupteur dans Connexions).
fn enable_builtin(core: &Core, id: &str, on: bool) {
    match id {
        "zigbee2mqtt" => {
            if let Some(mut z) = core
                .settings
                .get::<Zigbee2MqttSettings>(settings::ZIGBEE2MQTT)
            {
                z.enabled = on && !z.host.trim().is_empty();
                let _ = core.settings.set(settings::ZIGBEE2MQTT, &z);
                core.apply_zigbee_settings();
            }
            if !on {
                let mut l: crate::zigbee_local::LocalSettings = core
                    .settings
                    .get(crate::zigbee_local::SETTINGS_KEY)
                    .unwrap_or_default();
                if l.enabled {
                    l.enabled = false;
                    let _ = core.settings.set(crate::zigbee_local::SETTINGS_KEY, &l);
                    let z = core.zigbee_local.clone();
                    tokio::spawn(async move {
                        let _ = z.stop().await;
                    });
                }
            }
        }
        "tuya" => {
            use crate::integrations::tuya;
            let mut t: tuya::TuyaSettings =
                core.settings.get(tuya::SETTINGS_KEY).unwrap_or_default();
            if !on || !t.devices.is_empty() {
                t.enabled = on;
                let _ = core.settings.set(tuya::SETTINGS_KEY, &t);
                core.apply_tuya_settings();
            }
        }
        "matter" => {
            use crate::integrations::matter;
            let mut m: matter::MatterSettings =
                core.settings.get(matter::SETTINGS_KEY).unwrap_or_default();
            if !on || !m.url.trim().is_empty() {
                m.enabled = on;
                let _ = core.settings.set(matter::SETTINGS_KEY, &m);
                core.apply_matter_settings();
            }
        }
        _ => {}
    }
}

pub async fn uninstall(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let mut s = core.plugin_settings();
    let p = s.installed.remove(&id).ok_or_else(|| not_found(&id))?;
    if id == "mqtt_discovery" && s.installed.values().any(|p| !p.builtin) {
        return Err(bad_request(
            "des plugins externes s'en servent : désinstallez-les d'abord",
        ));
    }
    core.settings
        .set(plugins::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    if p.builtin {
        enable_builtin(core, &id, false);
    } else {
        core.plugins.stop(&id);
        core.plugins.remove_files(&id);
    }
    core.apply_discovery();
    Ok(Json(json!({ "removed": id })))
}

#[derive(Deserialize)]
pub struct UpdateInput {
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    settings: Option<Map<String, Value>>,
}

pub async fn update(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let lang = lang(&headers);
    let mut s = core.plugin_settings();
    let p = s.installed.get_mut(&id).ok_or_else(|| not_found(&id))?;
    if let Some(new) = input.settings {
        let defs = p
            .manifest
            .as_ref()
            .map(|m| m.settings.clone())
            .unwrap_or_default();
        // Un mot de passe laissé masqué (« •••••• ») garde l'ancien.
        let mut merged = new.clone();
        for d in defs.iter().filter(|d| d.kind == "password") {
            if merged.get(&d.key).and_then(Value::as_str) == Some("••••••")
                && let Some(old) = p.settings.get(&d.key)
            {
                merged.insert(d.key.clone(), old.clone());
            }
        }
        p.settings = plugins::clean_settings(&defs, &merged).map_err(bad_request)?;
    }
    if let Some(on) = input.enabled {
        if on && !p.builtin {
            let defs = p
                .manifest
                .as_ref()
                .map(|m| m.settings.clone())
                .unwrap_or_default();
            plugins::clean_settings(&defs, &p.settings).map_err(bad_request)?;
        }
        p.enabled = on;
    }
    let p = p.clone();
    core.settings
        .set(plugins::SETTINGS_KEY, &s)
        .map_err(bad_request)?;
    if p.builtin {
        if input.enabled.is_some() {
            enable_builtin(core, &id, p.enabled);
        }
    } else {
        core.apply_external(&p);
    }
    core.apply_discovery();
    let store = core.plugins.store(None).await;
    Ok(Json(installed_view(core, &p, &store, &lang)))
}

pub async fn restart(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let s = core.plugin_settings();
    let p = s.installed.get(&id).ok_or_else(|| not_found(&id))?;
    if p.builtin {
        return Err(bad_request("connexion intégrée : rien à relancer"));
    }
    core.apply_external(p);
    Ok(Json(json!({ "restarted": id })))
}

pub async fn log(State(app): State<AppState>, Path(id): Path<String>) -> impl IntoResponse {
    Json(json!({ "lines": app.core().plugins.log(&id) }))
}
