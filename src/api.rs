//! API REST + WebSocket.
//!
//! - REST : configuration et commandes ponctuelles.
//! - WebSocket `/api/ws` : instantané à la connexion, puis flux d'événements en direct.
//!   Le client peut s'annoncer (`hello`) pour recevoir les notifications et envoyer des commandes.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;

use crate::Core;
use crate::accounts::{Role, User};
use crate::api_accounts as acc;
use crate::api_boards as boards;
use crate::api_controls as ctl;
use crate::api_devices as dev;
use crate::api_presence as pres;
use crate::api_security as sec;
use crate::api_zigbee as zb;
use crate::bus::Event;
use crate::model::Action;
use crate::notify::{NotifyRequest, TargetKind};
use crate::registry::{CommandError, MetaPatch};
use crate::scenes::{CaptureInput, SceneInput};
use crate::timers::{TimerInput, TimerOp};

pub(crate) const INDEX_HTML: &str = include_str!("../static/index.html");
const MANIFEST: &str = include_str!("../static/manifest.webmanifest");
const SERVICE_WORKER: &str = include_str!("../static/sw.js");
const ICON_192: &[u8] = include_bytes!("../static/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../static/icon-512.png");
const APPLE_ICON: &[u8] = include_bytes!("../static/apple-touch-icon.png");

fn png(bytes: &'static [u8]) -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=604800"),
        ],
        bytes,
    )
}

#[derive(Clone)]
pub struct AppState {
    core: Core,
    token: Option<Arc<str>>,
    started: Instant,
    port: u16,
}

impl AppState {
    pub fn core(&self) -> &Core {
        &self.core
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

pub struct ApiError(pub StatusCode, pub String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<CommandError> for ApiError {
    fn from(e: CommandError) -> Self {
        let code = match e {
            CommandError::NotFound(_) => StatusCode::NOT_FOUND,
            CommandError::NotControllable(_) => StatusCode::UNPROCESSABLE_ENTITY,
            CommandError::Stopped => StatusCode::SERVICE_UNAVAILABLE,
        };
        ApiError(code, e.to_string())
    }
}

/// La langue de celui qui demande : celle de l'interface (en-tête X-Heim-Lang), sinon celle
/// de son navigateur, sinon celle de la maison.
pub fn lang_of(headers: &HeaderMap) -> &'static str {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    crate::i18n::known(header("x-heim-lang"))
        .or_else(|| crate::i18n::known(header("accept-language")))
        .or_else(|| crate::i18n::known(Some(&crate::i18n::house_lang())))
        .unwrap_or("fr")
}

/// Les messages d'erreur (« {"error": "…"} ») dans la langue de celui qui les lira.
async fn translate_errors(req: Request, next: Next) -> Response {
    let lang = lang_of(req.headers());
    let res = next.run(req).await;
    if lang == "fr" || !(res.status().is_client_error() || res.status().is_server_error()) {
        return res;
    }
    let json = res
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|c| c.starts_with("application/json"));
    if !json {
        return res;
    }
    let (mut parts, body) = res.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 256 * 1024).await else {
        return parts.status.into_response();
    };
    let Ok(mut v) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, axum::body::Body::from(bytes));
    };
    if let Some(Value::String(s)) = v.get_mut("error") {
        *s = crate::i18n::tr(lang, s).into_owned();
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, axum::body::Body::from(v.to_string()))
}

pub fn bad_request(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
}

pub fn not_found(what: &str) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("{what} introuvable"))
}

pub type ApiResult<T> = Result<T, ApiError>;

pub fn router(core: Core, token: Option<String>, port: u16) -> Router {
    use crate::api_life as life;
    use crate::api_more as more;
    let state = AppState {
        core,
        token: token.filter(|t| !t.is_empty()).map(Arc::from),
        started: Instant::now(),
        port,
    };

    let api = Router::new()
        .route("/health", get(health))
        .route("/integrations", get(integrations))
        .route("/entities", get(list_entities))
        .route(
            "/entities/{id}",
            get(get_entity)
                .patch(patch_entity)
                .delete(more::delete_entity),
        )
        .route("/entities/{id}/command", post(command_entity))
        .route("/entities/{id}/turn_on_for", post(more::turn_on_for))
        .route(
            "/settings/{key}",
            get(more::get_setting).put(more::put_setting),
        )
        .route("/system", get(more::system))
        .route("/backup", get(more::backup))
        .route(
            "/restore",
            post(more::restore).layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024)),
        )
        .route("/rooms/rename", post(more::rename_room))
        .route("/automations", get(more::list_automations))
        .route(
            "/automations/{id}",
            put(more::put_automation).delete(more::delete_automation),
        )
        .route("/automations/{id}/run", post(more::run_automation))
        .route("/scenes", get(list_scenes))
        .route("/scenes/{id}", put(put_scene).delete(delete_scene))
        .route("/scenes/{id}/capture", post(capture_scene))
        .route("/scenes/{id}/activate", post(activate_scene))
        .route("/scenes/{id}/duplicate", post(life::duplicate_scene))
        .route("/scenes/snapshot", post(life::scene_snapshot))
        .route("/scenes/preview", post(life::scene_preview))
        .route("/scenes/preview/end", post(life::scene_preview_end))
        .route("/scenes/{id}/deactivate", post(deactivate_scene))
        .route("/timers", get(list_timers))
        .route("/timers/{id}", put(put_timer).delete(delete_timer))
        .route("/timers/{id}/control", post(control_timer))
        .route("/notify", post(send_notification))
        .route("/notify/targets", get(list_targets))
        .route(
            "/notify/targets/{id}",
            put(put_target).delete(delete_target),
        )
        .route("/notify/inbox/{target}", get(inbox))
        .route("/security", get(sec::get_security))
        .route("/security/config", put(sec::put_config))
        .route("/security/code", put(sec::put_code))
        .route("/security/arm", post(sec::arm))
        .route("/security/disarm", post(sec::disarm))
        .route("/security/test_sirens", post(sec::test_sirens))
        .route("/security/snapshot", post(sec::take_snapshot))
        .route("/security/snapshots/{name}", get(sec::get_snapshot))
        .route("/zigbee/pairing", get(sec::get_pairing))
        .route("/zigbee/permit_join", post(sec::permit_join))
        .route("/broker", get(zb::get_broker).put(zb::put_broker))
        .route("/broker/users", post(zb::add_broker_user))
        .route(
            "/broker/users/{name}",
            axum::routing::delete(zb::delete_broker_user),
        )
        .route("/zigbee/local", get(zb::get_local).put(zb::put_local))
        .route("/zigbee/local/restart", post(zb::restart_local))
        .route(
            "/zigbee/local/import",
            post(zb::import_local).layer(axum::extract::DefaultBodyLimit::max(32 * 1024 * 1024)),
        )
        .route("/auth/status", get(acc::status))
        .route("/auth/me", get(acc::me))
        .route("/auth/login", post(acc::login))
        .route("/auth/logout", post(acc::logout))
        .route("/auth/password", post(acc::change_password))
        .route("/users", get(acc::list_users).post(acc::create_user))
        .route(
            "/users/{id}",
            put(acc::update_user).delete(acc::delete_user),
        )
        .route("/push/key", get(acc::push_key))
        .route(
            "/push/subscribe",
            post(acc::push_subscribe).delete(acc::push_unsubscribe),
        )
        .route("/push/test", post(acc::push_test))
        .route("/presence", get(pres::get_presence))
        .route("/presence/config", put(pres::put_presence))
        .route("/presence/scan", post(pres::scan))
        .route("/remote", get(pres::remote))
        .route("/tuya", get(dev::get_tuya).put(dev::put_tuya))
        .route("/tuya/import", post(dev::import_tuya))
        .route("/tuya/scan", post(dev::scan_tuya))
        .route("/matter", get(dev::get_matter).put(dev::put_matter))
        .route("/matter/commission", post(dev::commission))
        .route("/matter/remove", post(dev::remove_node))
        .route("/update", get(more::get_update))
        .route("/update/check", post(more::check_update))
        .route("/update/settings", put(more::put_update_settings))
        .route("/update/install", post(more::install_update))
        .route("/system/restart", post(more::restart))
        .route("/system/reboot", post(life::reboot))
        .route("/system/poweroff", post(life::poweroff))
        .route("/automations/{id}/op", post(life::automation_op))
        .route("/logbook", get(life::logbook))
        .route("/history/{id}", get(life::series))
        .route("/groups", get(life::get_groups).put(life::put_groups))
        .route("/entities/{id}/battery_reset", post(life::battery_reset))
        .route("/backups", get(life::list_backups).post(life::backup_now))
        .route("/backups/{name}", get(life::download_backup))
        .route("/setup", get(pres::setup_status))
        .route("/setup/scan", post(pres::setup_scan))
        .route("/setup/wifi", post(pres::setup_wifi))
        .route("/setup/done", post(pres::setup_done))
        .route("/setup/step", post(pres::setup_step))
        .route("/setup/ethernet", post(pres::setup_ethernet))
        .route("/setup/hostname", post(pres::setup_hostname))
        .route("/setup/timezone", post(pres::setup_timezone))
        .route("/devices/{id}/controls", get(ctl::get_controls))
        .route("/devices/{id}/set", post(ctl::set_controls))
        .route("/devices/{id}/action", post(ctl::device_action))
        .route("/power_on", get(ctl::get_power_on).put(ctl::put_power_on))
        .route("/integrations/catalog", get(more::integrations_catalog))
        .route(
            "/integrations/{name}/enabled",
            put(more::set_integration_enabled),
        )
        .route(
            "/dashboards",
            get(boards::get_dashboards).put(boards::put_dashboards),
        )
        .route("/dashboards/{id}", put(boards::put_board))
        .route("/screens", get(boards::list_screens))
        .route(
            "/screens/{id}",
            put(boards::put_screen).delete(boards::delete_screen),
        )
        .route("/screens/{id}/config", get(boards::screen_config))
        .route("/screens/{id}/identify", post(boards::identify_screen))
        .route(
            "/appearance",
            get(boards::get_appearance).put(boards::put_house_appearance),
        )
        .route("/appearance/me", put(boards::put_my_appearance))
        .route("/users/{id}/access", put(boards::put_access))
        .route("/sun", get(boards::sun))
        .route("/ws", get(ws_upgrade))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth))
        .fallback(|| async { ApiError(StatusCode::NOT_FOUND, "route inconnue".into()) })
        .layer(middleware::from_fn(translate_errors));

    Router::new()
        .route("/", get(|| async { Html(INDEX_HTML) }))
        // L'application installable : manifeste, icônes, service worker (notifications push).
        .route(
            "/manifest.webmanifest",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "application/manifest+json")],
                    MANIFEST,
                )
            }),
        )
        .route(
            "/sw.js",
            get(|| async {
                (
                    [
                        (header::CONTENT_TYPE, "text/javascript"),
                        (header::CACHE_CONTROL, "no-cache"),
                    ],
                    SERVICE_WORKER,
                )
            }),
        )
        .route("/icon-192.png", get(|| async { png(ICON_192) }))
        .route("/icon-512.png", get(|| async { png(ICON_512) }))
        .route("/apple-touch-icon.png", get(|| async { png(APPLE_ICON) }))
        .nest("/api", api)
        // Portail captif : sur le réseau « Heim-XXXX », toutes les adresses mènent à l'assistant
        // (c'est ce qui fait s'ouvrir la page toute seule sur le téléphone).
        .fallback(get(|| async { Html(INDEX_HTML) }))
        .layer(middleware::from_fn(crate::websec::layer))
        .with_state(state)
}

/// Qui appelle : un compte, le jeton HEIM_TOKEN (admin), ou personne.
#[derive(Debug, Clone)]
pub struct Caller {
    pub role: Role,
    /// `None` : jeton HEIM_TOKEN, ou Heim encore sans compte.
    pub user: Option<User>,
    /// Requête arrivée en HTTPS (Tailscale, tunnel) : le cookie doit être `Secure`.
    pub secure: bool,
}

pub const SESSION_COOKIE: &str = "heim_session";

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|h| h.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Le jeton de session présenté par la requête (cookie, ou en-tête pour les scripts).
pub fn session_token(headers: &HeaderMap) -> Option<String> {
    cookie(headers, SESSION_COOKIE).or_else(|| {
        headers
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(str::to_string)
    })
}

pub fn is_secure(headers: &HeaderMap) -> bool {
    let proto_https = headers
        .get("x-forwarded-proto")
        .and_then(|h| h.to_str().ok())
        .is_some_and(|p| p.eq_ignore_ascii_case("https"));
    let ts_host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h.split(':').next().unwrap_or_default().ends_with(".ts.net"));
    proto_https || ts_host
}

fn identify(app: &AppState, headers: &HeaderMap, query: Option<&str>) -> Option<Caller> {
    let secure = is_secure(headers);
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let query_token = query.and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("token=")));
    if let Some(token) = app.token.as_deref() {
        let matches = |t: Option<&str>| t.is_some_and(|t| crate::accounts::same(t, token));
        if matches(bearer) || matches(query_token) {
            return Some(Caller {
                role: Role::Admin,
                user: None,
                secure,
            });
        }
    }
    if let Some(user) = session_token(headers).and_then(|t| app.core.accounts.session_user(&t)) {
        return Some(Caller {
            role: user.role,
            user: Some(user),
            secure,
        });
    }
    // Aucun compte, aucun jeton : Heim est ouvert, comme avant la 0.5.
    if app.token.is_none() && app.core.accounts.is_open() {
        return Some(Caller {
            role: Role::Admin,
            user: None,
            secure,
        });
    }
    None
}

/// Le rôle minimum pour chaque route. `None` : ouverte à tous (écran de connexion).
/// Tout ce qui n'est pas listé demande d'être admin.
pub fn required_role(method: &axum::http::Method, path: &str) -> Option<Role> {
    let path = path.strip_prefix("/api").unwrap_or(path);
    let seg: Vec<&str> = path.trim_matches('/').split('/').collect();
    let get = method == axum::http::Method::GET;
    let post = method == axum::http::Method::POST;
    let del = method == axum::http::Method::DELETE;
    let put = method == axum::http::Method::PUT;
    match seg.as_slice() {
        ["health"] | ["auth", "status"] | ["push", "key"] | ["setup"] => None,
        ["auth", "login"] if post => None,
        ["auth", "me"] | ["ws"] if get => Some(Role::Guest),
        ["auth", "logout"] | ["auth", "password"] if post => Some(Role::Guest),
        ["entities"]
        | ["entities", _]
        | ["integrations"]
        | ["scenes"]
        | ["timers"]
        | ["security"]
            if get =>
        {
            Some(Role::Guest)
        }
        ["settings", k] if get && (k.starts_with("ui.") || *k == "home") => Some(Role::Guest),
        ["devices", _, "controls"] if get => Some(Role::Guest),
        ["devices", _, "set"] if post => Some(Role::Member),
        ["notify", "inbox", _] if get => Some(Role::Guest),
        ["entities", _, "command"] | ["entities", _, "turn_on_for"] if post => Some(Role::Guest),
        ["scenes", _, "activate"] | ["scenes", _, "deactivate"] | ["timers", _, "control"]
            if post =>
        {
            Some(Role::Guest)
        }
        ["push", "subscribe"] if post || del => Some(Role::Guest),
        ["push", "test"] if post => Some(Role::Guest),
        ["security", "arm"] | ["security", "disarm"] if post => Some(Role::Member),
        ["security", "snapshots", _] if get => Some(Role::Member),
        ["automations"]
        | ["notify", "targets"]
        | ["presence"]
        | ["zigbee", "pairing"]
        | ["users"]
            if get =>
        {
            Some(Role::Member)
        }
        ["automations", _, "run"] | ["automations", _, "op"] if post => Some(Role::Member),
        ["history", _] if get => Some(Role::Guest),
        ["logbook"] | ["groups"] if get => Some(Role::Member),
        ["entities", _, "battery_reset"] if post => Some(Role::Member),
        ["dashboards"] | ["appearance"] | ["sun"] | ["screens", _, "config"] if get => {
            Some(Role::Guest)
        }
        ["dashboards", _] | ["appearance", "me"] if put => Some(Role::Guest),
        _ => Some(Role::Admin),
    }
}

async fn auth(State(app): State<AppState>, mut req: Request, next: Next) -> Response {
    let need = required_role(req.method(), req.uri().path());
    let caller = identify(&app, req.headers(), req.uri().query());
    match (need, caller) {
        (None, caller) => {
            if let Some(c) = caller {
                req.extensions_mut().insert(c);
            }
            next.run(req).await
        }
        (Some(need), Some(c)) if c.role >= need => {
            req.extensions_mut().insert(c);
            next.run(req).await
        }
        (Some(need), Some(_)) => ApiError(
            StatusCode::FORBIDDEN,
            format!(
                "réservé aux comptes {}",
                match need {
                    Role::Admin => "admin",
                    Role::Member => "membre ou admin",
                    Role::Guest => "connectés",
                }
            ),
        )
        .into_response(),
        (Some(_), None) => {
            ApiError(StatusCode::UNAUTHORIZED, "connexion requise".into()).into_response()
        }
    }
}

fn rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

async fn health(State(app): State<AppState>) -> Json<Value> {
    Json(json!({
        "name": "heim",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_s": app.started.elapsed().as_secs(),
        "memory_kib": rss_kib(),
    }))
}

async fn integrations(State(app): State<AppState>) -> Json<Value> {
    let map = app
        .core
        .integrations
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    Json(json!(map))
}

// ---- Entités ---------------------------------------------------------------

async fn list_entities(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core.registry.snapshot().await)
}

async fn get_entity(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    app.core
        .registry
        .get(&id)
        .await
        .map(Json)
        .ok_or_else(|| not_found(&id))
}

async fn patch_entity(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<MetaPatch>,
) -> ApiResult<impl IntoResponse> {
    Ok(Json(app.core.registry.update_meta(&id, patch).await?))
}

/// Un invité commande lumières, prises, groupes et scènes, mais ni les sirènes ni l'état des
/// automatismes (réarmer le « courrier », c'est réservé aux membres, comme partout ailleurs).
/// Un appareil en lecture seule pour ce compte : visible, pas commandable.
pub fn check_readonly(caller: &Caller, entity_id: &str) -> ApiResult<()> {
    if caller
        .user
        .as_ref()
        .is_some_and(|u| !u.may_control(entity_id))
    {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "cet appareil est en lecture seule pour votre compte".into(),
        ));
    }
    Ok(())
}

pub fn may_command(role: Role, entity_id: &str) -> bool {
    role >= Role::Member
        || !(entity_id.starts_with("siren.") || entity_id.starts_with("automation."))
}

async fn command_entity(
    axum::Extension(caller): axum::Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(action): Json<Action>,
) -> ApiResult<impl IntoResponse> {
    if !may_command(caller.role, &id) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "réservé aux membres".into(),
        ));
    }
    check_readonly(&caller, &id)?;
    let target = app
        .core
        .registry
        .command(&id, action, &crate::api_life::actor(&caller))
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "entity_id": id, "target": target })),
    ))
}

// ---- Scènes ----------------------------------------------------------------

async fn list_scenes(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core.scenes.list().await)
}

async fn put_scene(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SceneInput>,
) -> ApiResult<impl IntoResponse> {
    app.core
        .scenes
        .upsert(&id, input)
        .await
        .map(Json)
        .map_err(bad_request)
}

async fn capture_scene(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CaptureInput>,
) -> ApiResult<impl IntoResponse> {
    app.core
        .scenes
        .capture(&id, input)
        .await
        .map(Json)
        .map_err(bad_request)
}

async fn delete_scene(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if app.core.scenes.delete(&id).await {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&id))
    }
}

#[derive(Deserialize, Default)]
struct ActivateInput {
    /// Activer pour tant de minutes, puis revenir comme avant.
    #[serde(default)]
    minutes: Option<u64>,
}

async fn activate_scene(
    axum::Extension(caller): axum::Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
    body: Option<Json<ActivateInput>>,
) -> ApiResult<StatusCode> {
    let minutes = body.and_then(|Json(b)| b.minutes);
    if minutes.is_some() {
        app.core
            .scenes
            .activate_for(&id, minutes)
            .await
            .map_err(bad_request)?;
        return Ok(StatusCode::ACCEPTED);
    }
    let action = Action::TurnOn {
        brightness: None,
        color_temp: None,
    };
    app.core
        .registry
        .command(
            &format!("scene.{id}"),
            action,
            &crate::api_life::actor(&caller),
        )
        .await?;
    Ok(StatusCode::ACCEPTED)
}

async fn deactivate_scene(
    axum::Extension(caller): axum::Extension<Caller>,
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    app.core
        .registry
        .command(
            &format!("scene.{id}"),
            Action::TurnOff,
            &crate::api_life::actor(&caller),
        )
        .await?;
    Ok(StatusCode::ACCEPTED)
}

// ---- Timers ----------------------------------------------------------------

async fn list_timers(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core.timers.list().await)
}

async fn put_timer(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TimerInput>,
) -> ApiResult<impl IntoResponse> {
    app.core
        .timers
        .upsert(&id, input)
        .await
        .map(Json)
        .map_err(bad_request)
}

async fn delete_timer(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if app.core.timers.delete(&id).await {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&id))
    }
}

async fn control_timer(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(op): Json<TimerOp>,
) -> ApiResult<impl IntoResponse> {
    app.core
        .timers
        .control(&id, op)
        .await
        .map(Json)
        .map_err(bad_request)
}

// ---- Notifications ---------------------------------------------------------

async fn send_notification(
    State(app): State<AppState>,
    Json(req): Json<NotifyRequest>,
) -> ApiResult<impl IntoResponse> {
    app.core.notify.send(req).map(Json).map_err(bad_request)
}

async fn list_targets(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core.notify.targets())
}

#[derive(Deserialize)]
struct TargetInput {
    name: String,
    #[serde(default)]
    person: Option<String>,
    #[serde(flatten)]
    kind: TargetKind,
}

async fn put_target(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<TargetInput>,
) -> ApiResult<impl IntoResponse> {
    app.core
        .notify
        .upsert_target(&id, input.name, input.person, input.kind)
        .map(Json)
        .map_err(bad_request)
}

async fn delete_target(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if app.core.notify.delete_target(&id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(not_found(&id))
    }
}

#[derive(Deserialize)]
struct InboxQuery {
    #[serde(default)]
    after: u64,
}

async fn inbox(
    State(app): State<AppState>,
    Path(target): Path<String>,
    Query(q): Query<InboxQuery>,
) -> impl IntoResponse {
    Json(app.core.notify.inbox(&target, q.after))
}

// ---- WebSocket -------------------------------------------------------------

async fn ws_upgrade(
    axum::Extension(caller): axum::Extension<Caller>,
    State(app): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.max_message_size(64 * 1024)
        .on_upgrade(move |socket| ws_session(socket, app, caller))
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    /// L'appareil s'annonce pour recevoir ses notifications.
    Hello {
        target_id: String,
        #[serde(default)]
        name: Option<String>,
        /// Ce que le navigateur dit être (« iPad »…), pour la page Écrans.
        #[serde(default)]
        kind: Option<String>,
        /// La langue de l'interface : les messages du Pi arrivent dans celle-ci.
        #[serde(default)]
        lang: Option<String>,
    },
    Command {
        entity_id: String,
        #[serde(flatten)]
        action: Action,
    },
    Ping,
}

async fn snapshot_msg(app: &AppState) -> Message {
    let entities = app.core.registry.snapshot().await;
    Message::Text(
        json!({ "type": "snapshot", "entities": entities })
            .to_string()
            .into(),
    )
}

async fn ws_session(socket: WebSocket, app: AppState, caller: Caller) {
    let (mut tx, mut rx) = socket.split();
    let mut events = app.core.bus.subscribe();
    let mut target_id: Option<String> = None;
    let mut screen_id: Option<String> = None;
    let mut lang: &'static str =
        crate::i18n::known(Some(&crate::i18n::house_lang())).unwrap_or("fr");
    let tr = |lang: &str, s: &str| crate::i18n::tr(lang, s).into_owned();

    if tx.send(snapshot_msg(&app).await).await.is_err() {
        return;
    }

    loop {
        tokio::select! {
            ev = events.recv() => {
                let out = match ev {
                    Ok(Event::CommandRequested { .. } | Event::CommandNoted { .. }) => continue,
                    Ok(Event::Notification { target_id: to, notification }) => {
                        if target_id.as_deref() != Some(to.as_str()) {
                            continue;
                        }
                        let mut n = serde_json::to_value(&notification).unwrap_or_default();
                        for k in ["title", "message"] {
                            if let Some(Value::String(s)) = n.get_mut(k) {
                                *s = tr(lang, s);
                            }
                        }
                        Message::Text(json!({ "type": "notification", "notification": n }).to_string().into())
                    }
                    Ok(event) => match serde_json::to_value(&event) {
                        Ok(mut v) => {
                            // Les textes du Pi (erreurs, état des connexions) dans la langue de l'écran.
                            if lang != "fr" {
                                for k in ["detail", "error", "message"] {
                                    if let Some(Value::String(s)) = v.get_mut(k) {
                                        *s = tr(lang, s);
                                    }
                                }
                            }
                            Message::Text(v.to_string().into())
                        }
                        Err(_) => continue,
                    },
                    // Trop lent pour suivre : on renvoie un instantané complet plutôt que des trous.
                    Err(RecvError::Lagged(_)) => snapshot_msg(&app).await,
                    Err(RecvError::Closed) => break,
                };
                if tx.send(out).await.is_err() {
                    break;
                }
            }
            msg = rx.next() => {
                let text = match msg {
                    Some(Ok(Message::Text(t))) => t,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => continue,
                };
                let reply = match serde_json::from_str::<ClientMsg>(text.as_str()) {
                    Ok(ClientMsg::Hello { target_id: id, name, kind, lang: l }) => {
                        if let Some(l) = crate::i18n::known(l.as_deref()) {
                            lang = l;
                        }
                        if let Some(old) = target_id.take() {
                            app.core.notify.app_disconnected(&old);
                        }
                        if let Some(old) = screen_id.take() {
                            app.core.screens.disconnected(&old);
                        }
                        let person = caller.user.as_ref().map(|u| u.id.clone());
                        app.core.screens.connected(&id, name.clone(), kind.map(|k| k.chars().take(40).collect()), caller.user.as_ref().map(|u| u.name.clone()));
                        screen_id = Some(id.clone());
                        target_id = app.core.notify.app_connected(&id, name, person);
                        json!({ "type": "welcome", "target_id": target_id, "role": caller.role, "user": caller.user })
                    }
                    Ok(ClientMsg::Command { entity_id, .. }) if !may_command(caller.role, &entity_id) => {
                        json!({ "type": "error", "entity_id": entity_id, "error": tr(lang, "réservé aux membres") })
                    }
                    Ok(ClientMsg::Command { entity_id, .. }) if caller.user.as_ref().and_then(|u| app.core.accounts.get(&u.id)).is_some_and(|u| !u.may_control(&entity_id)) => {
                        json!({ "type": "error", "entity_id": entity_id, "error": tr(lang, "cet appareil est en lecture seule pour votre compte") })
                    }
                    Ok(ClientMsg::Command { entity_id, action }) => match app.core.registry.command(&entity_id, action, &crate::api_life::actor(&caller)).await {
                        Ok(target) => json!({ "type": "ack", "entity_id": entity_id, "target": target }),
                        Err(e) => json!({ "type": "error", "entity_id": entity_id, "error": tr(lang, &e.to_string()) }),
                    },
                    Ok(ClientMsg::Ping) => json!({ "type": "pong" }),
                    Err(e) => json!({ "type": "error", "error": tr(lang, &format!("message illisible : {e}")) }),
                };
                if tx.send(Message::Text(reply.to_string().into())).await.is_err() {
                    break;
                }
            }
        }
    }
    if let Some(id) = target_id {
        app.core.notify.app_disconnected(&id);
    }
    if let Some(id) = screen_id {
        app.core.screens.disconnected(&id);
    }
}
