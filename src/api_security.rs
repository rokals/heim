//! Routes de la v0.4 : alarme et appairage Zigbee.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppState, bad_request, not_found};
use crate::bus::{Event, IntegrationState};
use crate::security::{self, ArmError, Mode, SecurityConfig};

// ---- Alarme -----------------------------------------------------------------------

/// État, réglages (sans le code), historique et photos en un seul appel.
pub async fn get_security(State(app): State<AppState>) -> impl IntoResponse {
    let sec = &app.core().security;
    Json(json!({
        "status": sec.status().await,
        "config": sec.config().public(),
        "log": sec.log(),
        "snapshots": sec.snapshots(),
    }))
}

pub async fn put_config(
    State(app): State<AppState>,
    Json(mut value): Json<Value>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    if let Some(o) = value.as_object_mut() {
        o.remove("has_code");
        o.remove("code");
    }
    let mut cfg: SecurityConfig = serde_json::from_value(value)
        .map_err(|e| bad_request(format!("réglage invalide : {e}")))?;
    cfg.validate().map_err(bad_request)?;
    // Le code se change à part, avec l'ancien.
    cfg.code = core.security.config().code;
    core.settings
        .set(security::CONFIG_KEY, &cfg)
        .map_err(bad_request)?;
    Ok(Json(cfg.public()))
}

#[derive(Deserialize)]
pub struct CodeInput {
    #[serde(default)]
    current: Option<String>,
    /// Vide ou absent : on retire le code.
    #[serde(default)]
    new: Option<String>,
}

pub async fn put_code(
    State(app): State<AppState>,
    Json(input): Json<CodeInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let mut cfg = core.security.config();
    if let Some(existing) = &cfg.code {
        let given = input.current.unwrap_or_default();
        if security::hash_code(given.trim(), &existing.salt) != existing.hash {
            return Err(ApiError(StatusCode::FORBIDDEN, "code actuel erroné".into()));
        }
    }
    cfg.code = match input.new.as_deref().map(str::trim) {
        Some(n) if !n.is_empty() => Some(security::new_code(n).map_err(bad_request)?),
        _ => None,
    };
    let has = cfg.code.is_some();
    core.settings
        .set(security::CONFIG_KEY, &cfg)
        .map_err(bad_request)?;
    Ok(Json(json!({ "has_code": has })))
}

#[derive(Deserialize)]
pub struct ArmInput {
    mode: Mode,
    #[serde(default)]
    force: bool,
    #[serde(default)]
    code: Option<String>,
}

pub async fn arm(State(app): State<AppState>, Json(input): Json<ArmInput>) -> Response {
    match app
        .core()
        .security
        .arm(input.mode, input.force, input.code)
        .await
    {
        Ok(status) => Json(status).into_response(),
        Err(e) => {
            let code = match &e {
                ArmError::NotReady { .. } => StatusCode::CONFLICT,
                ArmError::Code { .. } => StatusCode::FORBIDDEN,
                ArmError::Invalid { .. } => StatusCode::BAD_REQUEST,
            };
            let mut body = serde_json::to_value(&e).unwrap_or_default();
            if let Some(o) = body.as_object_mut() {
                o.insert("error".into(), json!(e.message()));
            }
            (code, Json(body)).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct DisarmInput {
    #[serde(default)]
    code: Option<String>,
}

pub async fn disarm(
    State(app): State<AppState>,
    Json(input): Json<DisarmInput>,
) -> ApiResult<impl IntoResponse> {
    app.core()
        .security
        .disarm(input.code)
        .await
        .map(Json)
        .map_err(|e| ApiError(StatusCode::FORBIDDEN, e))
}

#[derive(Deserialize)]
pub struct TestInput {
    #[serde(default)]
    seconds: Option<u32>,
}

pub async fn test_sirens(
    State(app): State<AppState>,
    Json(input): Json<TestInput>,
) -> ApiResult<impl IntoResponse> {
    let n = app
        .core()
        .security
        .test_sirens(input.seconds.unwrap_or(3))
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "sirens": n })))
}

#[derive(Deserialize)]
pub struct SnapshotInput {
    #[serde(default)]
    camera: Option<String>,
}

pub async fn take_snapshot(
    State(app): State<AppState>,
    Json(input): Json<SnapshotInput>,
) -> ApiResult<impl IntoResponse> {
    app.core()
        .security
        .snapshot(input.camera)
        .await
        .map(|names| Json(json!({ "snapshots": names })))
        .map_err(bad_request)
}

pub async fn get_snapshot(
    State(app): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if !security::valid_snapshot_name(&name) {
        return Err(not_found(&name));
    }
    let path = app.core().security.snapshots_dir().join(&name);
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| not_found(&name))?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "private, max-age=86400"),
        ],
        bytes,
    ))
}

// ---- Appairage Zigbee -------------------------------------------------------------

pub async fn get_pairing(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    let pairing = core
        .pairing
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Json(pairing)
}

#[derive(Deserialize)]
pub struct PermitInput {
    /// 0 pour fermer. 254 au plus (limite de Zigbee2MQTT).
    seconds: u32,
}

pub async fn permit_join(
    State(app): State<AppState>,
    Json(input): Json<PermitInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let connected = core
        .integrations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(crate::integrations::zigbee2mqtt::INTEGRATION)
        .and_then(|v| v.get("status").cloned())
        == Some(serde_json::to_value(IntegrationState::Connected).unwrap_or_default());
    if !connected {
        return Err(bad_request(
            "Zigbee2MQTT n'est pas connecté : vérifiez Réglages > Connexion Zigbee",
        ));
    }
    let seconds = input.seconds.min(254);
    if seconds > 0 {
        // Nouvel appairage : on repart d'une liste vide.
        core.pairing
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .events
            .clear();
    }
    core.bus.publish(Event::ZigbeePermitJoin { seconds });
    Ok((StatusCode::ACCEPTED, Json(json!({ "seconds": seconds }))))
}
