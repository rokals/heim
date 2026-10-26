//! Routes de la 0.12 : tableaux de bord multiples, écrans (tablettes), apparence, droits des
//! comptes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::accounts::{Access, Appearance, Role};
use crate::api::{ApiError, ApiResult, AppState, Caller, bad_request, not_found};
use crate::dashboards::{self, Boards};
use crate::screens::ScreenPatch;

pub const APPEARANCE_KEY: &str = "appearance";
/// Clé envoyée aux écrans quand un compte change : ils relisent leurs droits.
pub const ACCOUNTS_CHANGED: &str = "accounts";

fn forbidden(msg: &str) -> ApiError {
    ApiError(StatusCode::FORBIDDEN, msg.into())
}

fn access_of(caller: &Caller) -> Access {
    caller
        .user
        .as_ref()
        .map(|u| u.access.clone())
        .unwrap_or_default()
}

// ---- Tableaux de bord ------------------------------------------------------------

pub async fn get_dashboards(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
) -> impl IntoResponse {
    let all = dashboards::load(&app.core().settings);
    Json(dashboards::view(&all, caller.role, &access_of(&caller)))
}

/// L'admin enregistre la liste complète (créer, renommer, cacher, ordonner, supprimer).
pub async fn put_dashboards(
    State(app): State<AppState>,
    Json(input): Json<Boards>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let before = dashboards::load(&core.settings);
    let clean = dashboards::validate(input).map_err(bad_request)?;
    for gone in before
        .boards
        .iter()
        .filter(|b| !clean.boards.iter().any(|n| n.id == b.id))
    {
        core.screens.forget_board(&gone.id);
    }
    core.settings
        .set(dashboards::KEY, &clean)
        .map_err(bad_request)?;
    Ok(Json(clean))
}

#[derive(Debug, Deserialize)]
pub struct SectionsInput {
    pub sections: Vec<Value>,
}

/// Les cartes d'un tableau : l'admin toujours, les autres selon les droits de leur compte.
pub async fn put_board(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SectionsInput>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let mut all = dashboards::load(&core.settings);
    let access = access_of(&caller);
    let view = dashboards::view(&all, caller.role, &access);
    if !view.boards.iter().any(|b| b.id == id) {
        return Err(not_found(&id));
    }
    let board = all
        .boards
        .iter_mut()
        .find(|b| b.id == id)
        .ok_or_else(|| not_found(&id))?;
    if caller.role != Role::Admin {
        dashboards::check_change(
            &board.sections,
            &input.sections,
            access.can_edit,
            access.can_add,
        )
        .map_err(|e| forbidden(&e))?;
    }
    board.sections = input.sections;
    board.auto = false;
    let clean = dashboards::validate(all).map_err(bad_request)?;
    core.settings
        .set(dashboards::KEY, &clean)
        .map_err(bad_request)?;
    let saved = clean.boards.into_iter().find(|b| b.id == id);
    Ok(Json(saved))
}

// ---- Écrans ----------------------------------------------------------------------

pub async fn list_screens(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core().screens.list())
}

/// Les réglages de l'écran qui demande (son identifiant vient du navigateur).
pub async fn screen_config(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    app.core()
        .screens
        .get(&id)
        .map(Json)
        .ok_or_else(|| not_found(&id))
}

pub async fn put_screen(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<ScreenPatch>,
) -> ApiResult<impl IntoResponse> {
    if let Some(Some(a)) = &patch.appearance {
        check_appearance(a)?;
    }
    app.core()
        .screens
        .update(&id, patch)
        .map(Json)
        .map_err(bad_request)
}

pub async fn delete_screen(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if app.core().screens.remove(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&id))
    }
}

pub async fn identify_screen(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    if app.core().screens.identify(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&id))
    }
}

// ---- Apparence -------------------------------------------------------------------

fn check_appearance(a: &Appearance) -> ApiResult<()> {
    let size = serde_json::to_vec(a).map(|v| v.len()).unwrap_or(usize::MAX);
    if a.len() > 30 || size > 4096 {
        return Err(bad_request("apparence trop volumineuse"));
    }
    Ok(())
}

/// L'apparence de la maison (par défaut) et celle du compte connecté.
pub async fn get_appearance(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
) -> impl IntoResponse {
    let house = app
        .core()
        .settings
        .get_raw(APPEARANCE_KEY)
        .unwrap_or_else(|| json!({}));
    let mine = caller.user.as_ref().and_then(|u| u.appearance.clone());
    Json(json!({ "house": house, "mine": mine }))
}

/// Chacun règle son apparence ; sans compte, c'est celle de la maison.
pub async fn put_my_appearance(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Json(input): Json<Option<Appearance>>,
) -> ApiResult<impl IntoResponse> {
    if let Some(a) = &input {
        check_appearance(a)?;
    }
    let core = app.core();
    match &caller.user {
        Some(u) => {
            core.accounts
                .set_appearance(&u.id, input)
                .map_err(bad_request)?;
            core.settings.touch(ACCOUNTS_CHANGED);
        }
        None => {
            core.settings
                .set(APPEARANCE_KEY, &input.unwrap_or_default())
                .map_err(bad_request)?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn put_house_appearance(
    State(app): State<AppState>,
    Json(input): Json<Appearance>,
) -> ApiResult<impl IntoResponse> {
    check_appearance(&input)?;
    app.core()
        .settings
        .set(APPEARANCE_KEY, &input)
        .map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Droits des comptes ----------------------------------------------------------

pub async fn put_access(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<Access>,
) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    let user = core.accounts.set_access(&id, input).map_err(bad_request)?;
    core.settings.touch(ACCOUNTS_CHANGED);
    Ok(Json(user))
}

// ---- Soleil (carte « Soleil » de l'accueil) ------------------------------------------

/// Lever et coucher d'aujourd'hui et de demain, au lieu réglé dans Réglages > Lieu.
pub async fn sun(State(app): State<AppState>) -> impl IntoResponse {
    let loc: crate::settings::Location = app
        .core()
        .settings
        .get_or_default(crate::settings::LOCATION);
    let today = chrono::Utc::now().date_naive();
    let days: Vec<Value> = [today, today + chrono::Days::new(1)]
        .iter()
        .map(
            |d| match crate::sun::sun_times(*d, loc.latitude, loc.longitude) {
                Some((rise, set)) => json!({ "rise": rise, "set": set }),
                None => json!(null),
            },
        )
        .collect();
    Json(json!({ "days": days }))
}
