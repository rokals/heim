//! Routes de la v0.7 : réglages complets des appareils, état après coupure pour tous.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::api::{ApiResult, AppState, bad_request, not_found};
use crate::bus::Event;
use crate::devices::{self, DeviceControls};

pub const POWER_ON_KEY: &str = "power_on";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PowerOnSettings {
    /// « previous », « on », « off » ; vide : chaque appareil garde le sien.
    #[serde(default)]
    pub behavior: Option<String>,
    /// Régler aussi les appareils ajoutés ensuite.
    #[serde(default)]
    pub apply_new: bool,
}

pub async fn get_controls(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let d = devices::lock(&core.controls)
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(&id))?;
    let supported = devices::power_on_supported(&d);
    let mut v = serde_json::to_value(&d).unwrap_or_default();
    v["power_on"] = json!(supported);
    Ok(Json(v))
}

#[derive(Deserialize)]
pub struct SetInput {
    values: Map<String, Value>,
}

/// N'accepte que des réglages que l'appareil a annoncés, avec le bon type.
fn check(d: &DeviceControls, values: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in values {
        if k == "transition" {
            if !v.is_number() {
                return Err("durée de fondu invalide".into());
            }
            continue;
        }
        let Some(c) = d
            .controls
            .iter()
            .find(|c| c.get("key").and_then(Value::as_str) == Some(k))
        else {
            return Err(format!("réglage « {k} » inconnu pour cet appareil"));
        };
        let kind = c.get("kind").and_then(Value::as_str).unwrap_or_default();
        let ok = match kind {
            "binary" => {
                [c.get("value_on"), c.get("value_off"), c.get("value_toggle")]
                    .into_iter()
                    .flatten()
                    .any(|x| x == v)
                    || (c.get("value_on").is_none() && v.is_boolean())
            }
            "enum" => c
                .get("values")
                .and_then(Value::as_array)
                .is_some_and(|vals| vals.contains(v)),
            "numeric" => v.as_f64().is_some_and(|n| {
                let min = c.get("min").and_then(Value::as_f64).unwrap_or(f64::MIN);
                let max = c.get("max").and_then(Value::as_f64).unwrap_or(f64::MAX);
                // Les préréglages (« comme avant » = 65535...) peuvent sortir de la plage.
                let preset = c.get("presets").and_then(Value::as_array).is_some_and(|p| {
                    p.iter()
                        .any(|x| x.get("value").and_then(Value::as_f64) == Some(n))
                });
                (n >= min && n <= max) || preset
            }),
            "text" => v.is_string(),
            "color" => v.is_object(),
            _ => false,
        };
        if !ok {
            return Err(format!("valeur refusée pour « {k} »"));
        }
    }
    Ok(())
}

pub async fn set_controls(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SetInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let d = devices::lock(&core.controls)
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(&id))?;
    if input.values.is_empty() {
        return Err(bad_request("aucun réglage"));
    }
    check(&d, &input.values).map_err(bad_request)?;
    core.bus.publish(Event::DeviceSet {
        integration: d.integration.clone(),
        device_id: id,
        values: input.values,
    });
    Ok(StatusCode::ACCEPTED)
}

#[derive(Deserialize)]
pub struct ActionInput {
    action: String,
    #[serde(default)]
    force: bool,
}

pub async fn device_action(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ActionInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let d = devices::lock(&core.controls)
        .get(&id)
        .cloned()
        .ok_or_else(|| not_found(&id))?;
    if !matches!(
        input.action.as_str(),
        "remove" | "ota_check" | "ota_update" | "interview" | "read_battery"
    ) {
        return Err(bad_request("action inconnue"));
    }
    if d.integration != crate::integrations::zigbee2mqtt::INTEGRATION {
        return Err(bad_request("action réservée aux appareils Zigbee"));
    }
    core.bus.publish(Event::DeviceAction {
        integration: d.integration,
        device_id: id,
        action: input.action,
        force: input.force,
    });
    Ok(StatusCode::ACCEPTED)
}

// ---- Après une coupure de courant : pour tous les appareils ------------------------

pub async fn get_power_on(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let settings: PowerOnSettings = core.settings.get(POWER_ON_KEY).unwrap_or_default();
    let list: Vec<Value> = devices::lock(&core.controls)
        .values()
        .map(|d| {
            let current = d
                .controls
                .iter()
                .find(|c| {
                    devices::power_on_value(c, "previous").is_some()
                        || devices::power_on_value(c, "off").is_some()
                })
                .and_then(|c| c.get("key").and_then(Value::as_str))
                .and_then(|k| devices::value_at(&d.values, k).cloned());
            json!({
                "device_id": d.device_id,
                "name": d.name,
                "integration": d.integration,
                "supported": devices::power_on_supported(d),
                "current": current,
            })
        })
        .collect();
    Json(json!({ "settings": settings, "devices": list }))
}

/// Applique un comportement à tous les appareils qui le permettent. Renvoie (réglés, ignorés).
pub fn apply_power_on(
    core: &crate::Core,
    behavior: &str,
    only: Option<&str>,
) -> (usize, Vec<String>) {
    let mut applied = 0;
    let mut skipped = Vec::new();
    let targets: Vec<DeviceControls> = devices::lock(&core.controls)
        .values()
        .filter(|d| only.is_none_or(|o| d.device_id == o || d.name == o))
        .cloned()
        .collect();
    for d in targets {
        let payload = devices::power_on_payload(&d, behavior);
        if payload.is_empty() {
            if !devices::power_on_supported(&d).is_empty() {
                skipped.push(d.name.clone());
            }
            continue;
        }
        core.bus.publish(Event::DeviceSet {
            integration: d.integration.clone(),
            device_id: d.device_id.clone(),
            values: payload,
        });
        applied += 1;
    }
    (applied, skipped)
}

pub async fn put_power_on(
    State(app): State<AppState>,
    Json(input): Json<PowerOnSettings>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    if let Some(b) = &input.behavior
        && !matches!(b.as_str(), "previous" | "on" | "off")
    {
        return Err(bad_request("choix attendu : previous, on ou off"));
    }
    core.settings
        .set(POWER_ON_KEY, &input)
        .map_err(bad_request)?;
    let (applied, skipped) = match &input.behavior {
        Some(b) => apply_power_on(core, b, None),
        None => (0, Vec::new()),
    };
    Ok(Json(
        json!({ "applied": applied, "skipped": skipped, "settings": input }),
    ))
}
