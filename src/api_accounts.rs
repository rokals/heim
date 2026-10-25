//! Routes de la v0.5 : connexion, comptes.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use serde_json::json;

use crate::accounts::Role;
use crate::api::{
    ApiError, ApiResult, AppState, Caller, SESSION_COOKIE, bad_request, session_token,
};

fn set_cookie(token: &str, secure: bool) -> String {
    format!(
        "{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=31536000{}",
        if secure { "; Secure" } else { "" }
    )
}

fn clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

fn device_label(headers: &HeaderMap) -> Option<String> {
    let ua = headers.get(header::USER_AGENT)?.to_str().ok()?;
    let label = if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("Android") {
        if ua.contains("Mobile") {
            "Téléphone Android"
        } else {
            "Tablette Android"
        }
    } else if ua.contains("Windows") {
        "Ordinateur Windows"
    } else if ua.contains("Mac OS") {
        "Mac"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        "Navigateur"
    };
    Some(label.to_string())
}

/// Public : ce qu'il faut pour afficher l'écran de connexion.
pub async fn status(State(app): State<AppState>) -> impl IntoResponse {
    let accounts = &app.core().accounts;
    let names: Vec<_> = accounts
        .names()
        .into_iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Json(json!({ "open": accounts.is_open(), "users": names }))
}

pub async fn me(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
) -> impl IntoResponse {
    let sessions = caller
        .user
        .as_ref()
        .map(|u| app.core().accounts.sessions_of(&u.id))
        .unwrap_or_default();
    Json(json!({
        "user": caller.user,
        "role": caller.role,
        "open": app.core().accounts.is_open(),
        "secure": caller.secure,
        "sessions": sessions,
    }))
}

#[derive(Deserialize)]
pub struct LoginInput {
    name: String,
    password: String,
}

pub async fn login(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<LoginInput>,
) -> ApiResult<Response> {
    let accounts = app.core().accounts.clone();
    let label = device_label(&headers);
    let (token, user) =
        tokio::task::spawn_blocking(move || accounts.login(&input.name, &input.password, label))
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| ApiError(StatusCode::UNAUTHORIZED, e))?;
    Ok((
        [(
            header::SET_COOKIE,
            set_cookie(&token, crate::api::is_secure(&headers)),
        )],
        Json(json!({ "user": user })),
    )
        .into_response())
}

pub async fn logout(State(app): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(token) = session_token(&headers) {
        app.core().accounts.logout(&token);
    }
    (
        [(header::SET_COOKIE, clear_cookie())],
        StatusCode::NO_CONTENT,
    )
}

#[derive(Deserialize)]
pub struct PasswordInput {
    current: String,
    new: String,
}

pub async fn change_password(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Json(input): Json<PasswordInput>,
) -> ApiResult<impl IntoResponse> {
    let user = caller
        .user
        .ok_or_else(|| bad_request("aucun compte connecté"))?;
    let accounts = app.core().accounts.clone();
    let id = user.id.clone();
    let ok = tokio::task::spawn_blocking(move || accounts.verify(&id, &input.current))
        .await
        .unwrap_or(false);
    if !ok {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "mot de passe actuel incorrect".into(),
        ));
    }
    let accounts = app.core().accounts.clone();
    tokio::task::spawn_blocking(move || accounts.update(&user.id, None, None, Some(input.new)))
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map_err(bad_request)?;
    Ok(Json(json!({ "relogin": true })))
}

// ---- Gestion des comptes (admin) -------------------------------------------------

pub async fn list_users(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core().accounts.list())
}

#[derive(Deserialize)]
pub struct NewUser {
    name: String,
    #[serde(default = "member")]
    role: Role,
    password: String,
}

fn member() -> Role {
    Role::Member
}

/// Crée un compte. Le tout premier connecte aussitôt ce navigateur (sinon on s'enfermerait dehors).
pub async fn create_user(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<NewUser>,
) -> ApiResult<Response> {
    let accounts = app.core().accounts.clone();
    let first = accounts.is_open();
    let label = device_label(&headers);
    let (user, token) = tokio::task::spawn_blocking(move || {
        let user = accounts.create(&input.name, input.role, &input.password)?;
        let token = first.then(|| accounts.open_session(&user.id, label));
        Ok::<_, String>((user, token))
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(bad_request)?;
    let body = Json(json!({ "user": user, "logged_in": token.is_some() }));
    Ok(match token {
        Some(t) => (
            [(
                header::SET_COOKIE,
                set_cookie(&t, crate::api::is_secure(&headers)),
            )],
            body,
        )
            .into_response(),
        None => body.into_response(),
    })
}

#[derive(Deserialize)]
pub struct UserPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    role: Option<Role>,
    #[serde(default)]
    password: Option<String>,
}

pub async fn update_user(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<UserPatch>,
) -> ApiResult<impl IntoResponse> {
    let accounts = app.core().accounts.clone();
    tokio::task::spawn_blocking(move || {
        accounts.update(
            &id,
            patch.name,
            patch.role,
            patch.password.filter(|p| !p.is_empty()),
        )
    })
    .await
    .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map(Json)
    .map_err(bad_request)
}

pub async fn delete_user(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if caller.user.as_ref().is_some_and(|u| u.id == id) {
        return Err(bad_request(
            "vous ne pouvez pas supprimer votre propre compte",
        ));
    }
    app.core().accounts.delete(&id).map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Notifications push ------------------------------------------------------------

pub async fn push_key(State(app): State<AppState>) -> impl IntoResponse {
    Json(json!({ "public_key": app.core().notify.vapid_public() }))
}

#[derive(Deserialize)]
pub struct SubscribeInput {
    target_id: String,
    subscription: crate::push::Subscription,
}

/// Un compte ne touche qu'aux notifications de ses propres appareils (l'admin, à tous).
fn may_touch_target(app: &AppState, caller: &Caller, target_id: &str) -> ApiResult<()> {
    if caller.role == Role::Admin {
        return Ok(());
    }
    match (app.core().notify.target_owner(target_id), &caller.user) {
        (None, _) => Ok(()),
        (Some(owner), Some(u)) if owner == u.id => Ok(()),
        _ => Err(ApiError(
            StatusCode::FORBIDDEN,
            "cet appareil appartient à un autre compte".into(),
        )),
    }
}

pub async fn push_subscribe(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Json(input): Json<SubscribeInput>,
) -> ApiResult<impl IntoResponse> {
    may_touch_target(&app, &caller, &input.target_id)?;
    app.core()
        .notify
        .subscribe(&input.target_id, input.subscription)
        .map(Json)
        .map_err(bad_request)
}

#[derive(Deserialize)]
pub struct TargetInput {
    target_id: String,
}

pub async fn push_unsubscribe(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Json(input): Json<TargetInput>,
) -> ApiResult<impl IntoResponse> {
    may_touch_target(&app, &caller, &input.target_id)?;
    app.core().notify.unsubscribe(&input.target_id);
    Ok(StatusCode::NO_CONTENT)
}

pub async fn push_test(
    Extension(caller): Extension<Caller>,
    State(app): State<AppState>,
    Json(input): Json<TargetInput>,
) -> ApiResult<impl IntoResponse> {
    may_touch_target(&app, &caller, &input.target_id)?;
    app.core()
        .notify
        .push_test(&input.target_id)
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "sent": true })))
}
