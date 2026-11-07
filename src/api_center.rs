//! Le centre de notifications (la cloche) : lire, marquer lu, retirer, régler.

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde_json::json;

use crate::accounts::Role;
use crate::api::{ApiResult, AppState, Caller, bad_request};
use crate::center::{CenterSettings, SETTINGS_KEY, Viewer};

fn viewer(caller: &Option<axum::Extension<Caller>>) -> Viewer {
    match caller {
        Some(axum::Extension(c)) => Viewer {
            account: c.user.as_ref().map(|u| u.id.clone()),
            admin: c.role == Role::Admin,
        },
        None => Viewer {
            account: None,
            admin: false,
        },
    }
}

pub async fn get_center(
    State(app): State<AppState>,
    caller: Option<axum::Extension<Caller>>,
) -> impl IntoResponse {
    let v = viewer(&caller);
    let c = &app.core().center;
    Json(json!({
        "notes": c.list(&v),
        "unread": c.unread(&v),
        "seen_at": c.seen_at(&v),
    }))
}

pub async fn mark_seen(
    State(app): State<AppState>,
    caller: Option<axum::Extension<Caller>>,
) -> impl IntoResponse {
    app.core().center.mark_seen(&viewer(&caller));
    Json(json!({ "ok": true }))
}

pub async fn dismiss(
    State(app): State<AppState>,
    caller: Option<axum::Extension<Caller>>,
    Path(id): Path<u64>,
) -> impl IntoResponse {
    let n = app.core().center.dismiss(Some(id), &viewer(&caller));
    Json(json!({ "removed": n }))
}

pub async fn dismiss_all(
    State(app): State<AppState>,
    caller: Option<axum::Extension<Caller>>,
) -> impl IntoResponse {
    let n = app.core().center.dismiss(None, &viewer(&caller));
    Json(json!({ "removed": n }))
}

pub async fn get_settings(State(app): State<AppState>) -> impl IntoResponse {
    let s: CenterSettings = app.core().settings.get(SETTINGS_KEY).unwrap_or_default();
    Json(s)
}

pub async fn put_settings(
    State(app): State<AppState>,
    Json(mut input): Json<CenterSettings>,
) -> ApiResult<impl IntoResponse> {
    for (i, r) in input.energy.iter_mut().enumerate() {
        if r.id.trim().is_empty() {
            r.id = format!("r{}", chrono::Utc::now().timestamp_millis() + i as i64);
        }
    }
    input.validate().map_err(bad_request)?;
    app.core()
        .settings
        .set(SETTINGS_KEY, &input)
        .map_err(bad_request)?;
    Ok(Json(input))
}
