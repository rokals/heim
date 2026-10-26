//! API du quotidien : état des automatismes, journal, courbes, groupes, sauvegardes de nuit,
//! pile changée, arrêt de la machine.

use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::api::{ApiError, ApiResult, AppState, Caller, bad_request, not_found};
use crate::automations::OpKind;
use crate::bus::Event;
use crate::groups::{self, Group};
use crate::history::LogQuery;
use crate::model::Attributes;
use crate::registry::{STALE_SINCE, STALE_VALUE};
use crate::system::{self, Power};

/// Comment le journal nomme celui qui agit : son prénom, sinon rien de parlant.
pub fn actor(caller: &Caller) -> String {
    match &caller.user {
        Some(u) => format!("user:{}", u.name),
        None => "api".into(),
    }
}

fn log(app: &AppState, kind: &str, message: String, caller: &Caller) {
    app.core().bus.publish(Event::Log {
        kind: kind.into(),
        message,
        entity_id: None,
        actor: caller.user.as_ref().map(|u| u.name.clone()),
    });
}

// ---- État des automatismes ---------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct OpInput {
    /// Activer / désactiver.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// true / false, ou « rearm » pour revenir au repos.
    #[serde(default)]
    pub state: Option<Value>,
    /// Lancer ses actions tout de suite.
    #[serde(default)]
    pub run: bool,
}

pub async fn automation_op(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<OpInput>,
) -> ApiResult<impl IntoResponse> {
    let op = if input.run {
        OpKind::Run
    } else if let Some(e) = input.enabled {
        OpKind::Enabled(e)
    } else {
        match input.state {
            Some(Value::Bool(b)) => OpKind::State(Some(b)),
            Some(Value::String(s)) if s == "rearm" => OpKind::State(None),
            _ => {
                return Err(bad_request(
                    "précisez enabled, state (true, false, « rearm ») ou run",
                ));
            }
        }
    };
    let a = app
        .core()
        .automations
        .op(&id, op, &actor(&caller))
        .await
        .map_err(bad_request)?;
    let what = match op {
        OpKind::Enabled(true) => "activé".to_string(),
        OpKind::Enabled(false) => "désactivé".to_string(),
        OpKind::State(None) => "réarmé".to_string(),
        OpKind::State(Some(v)) => format!("état mis à {}", if v { "vrai" } else { "faux" }),
        OpKind::Run => "lancé à la main".to_string(),
    };
    app.core().bus.publish(Event::Log {
        kind: "automation".into(),
        message: what,
        entity_id: Some(a.entity_id()),
        actor: caller.user.as_ref().map(|u| u.name.clone()),
    });
    Ok(Json(a))
}

// ---- Journal et courbes ------------------------------------------------------------

pub async fn logbook(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(q): Query<LogQuery>,
) -> ApiResult<impl IntoResponse> {
    let history = app.core().history.clone();
    let mut rows = tokio::task::spawn_blocking(move || history.query(&q))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let lang = crate::api::lang_of(&headers);
    if lang != "fr" {
        for r in &mut rows {
            for s in [&mut r.message, &mut r.actor].into_iter().flatten() {
                *s = crate::i18n::tr(lang, s).into_owned();
            }
        }
    }
    Ok(Json(rows))
}

#[derive(Debug, Deserialize)]
pub struct SeriesQuery {
    #[serde(default)]
    pub hours: Option<i64>,
    #[serde(default)]
    pub points: Option<usize>,
}

pub async fn series(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<SeriesQuery>,
) -> ApiResult<impl IntoResponse> {
    let history = app.core().history.clone();
    let hours = q.hours.unwrap_or(24);
    let points = q.points.unwrap_or(200).clamp(10, 1000);
    let pts = tokio::task::spawn_blocking(move || history.series(&id, hours, points))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(pts))
}

// ---- Groupes -----------------------------------------------------------------------

pub async fn get_groups(State(app): State<AppState>) -> impl IntoResponse {
    let groups: Vec<Group> = app
        .core()
        .settings
        .get(groups::SETTINGS_KEY)
        .unwrap_or_default();
    Json(groups)
}

pub async fn put_groups(
    State(app): State<AppState>,
    Json(input): Json<Vec<Group>>,
) -> ApiResult<impl IntoResponse> {
    let clean = groups::validate(input).map_err(bad_request)?;
    app.core()
        .settings
        .set(groups::SETTINGS_KEY, &clean)
        .map_err(bad_request)?;
    Ok(Json(clean))
}

// ---- Pile changée ------------------------------------------------------------------

pub async fn battery_reset(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let e = core.registry.get(&id).await.ok_or_else(|| not_found(&id))?;
    let kind = e.meta.capabilities.get("kind").and_then(Value::as_str);
    if kind != Some("battery") {
        return Err(bad_request("ce n'est pas un niveau de pile"));
    }
    let state = e.state.as_ref();
    let mut attrs = Attributes::new();
    if let Some(old) = state.map(|s| s.value.clone()).filter(|v| !v.is_null()) {
        attrs.insert(STALE_VALUE.into(), old);
    }
    attrs.insert(STALE_SINCE.into(), json!(chrono::Utc::now()));
    let available = state.is_none_or(|s| s.available);
    core.registry
        .report(id.clone(), Value::Null, attrs, available)
        .await;
    core.bus.publish(Event::Log {
        kind: "state".into(),
        message: "pile changée".into(),
        entity_id: Some(id),
        actor: caller.user.as_ref().map(|u| u.name.clone()),
    });
    Ok(StatusCode::NO_CONTENT)
}

// ---- Sauvegardes de nuit -----------------------------------------------------------

pub async fn list_backups(State(app): State<AppState>) -> impl IntoResponse {
    Json(json!({
        "keep": system::KEEP_BACKUPS,
        "backups": system::list_backups(&app.core().data_dir),
    }))
}

pub async fn backup_now(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let (store, dir) = (core.store.clone(), core.data_dir.clone());
    let b = tokio::task::spawn_blocking(move || system::backup_now(&store, &dir))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(bad_request)?;
    Ok(Json(b))
}

pub async fn download_backup(
    State(app): State<AppState>,
    Path(name): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let path = system::backup_path(&app.core().data_dir, &name).ok_or_else(|| not_found(&name))?;
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
        .await
        .ok()
        .and_then(Result::ok)
        .ok_or_else(|| not_found(&name))?;
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

// ---- Arrêt et redémarrage ----------------------------------------------------------

async fn power(app: AppState, caller: Caller, what: Power) -> ApiResult<impl IntoResponse> {
    let (word, key) = match what {
        Power::Reboot => ("redémarrage du Raspberry Pi demandé", "rebooting"),
        Power::PowerOff => ("arrêt du Raspberry Pi demandé", "powering_off"),
    };
    log(&app, "system", word.into(), &caller);
    // Tout écrire avant que la machine ne s'arrête.
    let core = app.core();
    let (store, history) = (core.store.clone(), core.history.clone());
    tokio::task::spawn_blocking(move || {
        store.flush_blocking();
        history.flush();
    })
    .await
    .ok();
    system::power(what).await.map_err(bad_request)?;
    Ok(Json(json!({ key: true })))
}

pub async fn reboot(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    power(app, caller, Power::Reboot).await
}

pub async fn poweroff(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
) -> ApiResult<impl IntoResponse> {
    power(app, caller, Power::PowerOff).await
}

// ---- Scènes : éditeur ----------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SnapshotInput {
    pub entities: Vec<String>,
}

/// L'état actuel de ces appareils, comme réglages de scène (« Reprendre l'état actuel »).
pub async fn scene_snapshot(
    State(app): State<AppState>,
    Json(input): Json<SnapshotInput>,
) -> ApiResult<impl IntoResponse> {
    if input.entities.len() > 60 {
        return Err(bad_request("trop d'appareils"));
    }
    Ok(Json(app.core().scenes.snapshot(input.entities).await))
}

#[derive(Debug, Deserialize)]
pub struct PreviewInput {
    pub targets: std::collections::BTreeMap<String, crate::scenes::SceneTarget>,
    #[serde(default)]
    pub transition_s: Option<f64>,
}

/// Aperçu en direct : applique ces réglages sur les vraies lampes, sans rien enregistrer.
pub async fn scene_preview(
    State(app): State<AppState>,
    Json(input): Json<PreviewInput>,
) -> ApiResult<impl IntoResponse> {
    if input.targets.len() > 60 {
        return Err(bad_request("trop d'appareils"));
    }
    app.core()
        .scenes
        .preview(input.targets, input.transition_s)
        .await
        .map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Fin de l'aperçu : tout revient comme avant.
pub async fn scene_preview_end(State(app): State<AppState>) -> impl IntoResponse {
    Json(json!({ "restored": app.core().scenes.preview_end().await }))
}

pub async fn duplicate_scene(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    app.core()
        .scenes
        .duplicate(&id)
        .await
        .map(Json)
        .map_err(bad_request)
}
