//! Routes de la v0.5 : présence par le Wi-Fi, accès à distance (Tailscale).

use axum::Json;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use serde_json::{Value, json};

use crate::api::{ApiResult, AppState, bad_request};
use crate::presence::{self, PresenceConfig};

pub async fn get_presence(State(app): State<AppState>) -> impl IntoResponse {
    let p = &app.core().presence;
    Json(json!({
        "config": p.config(),
        "people": p.people().await,
        "local_ip": presence::local_ipv4(),
    }))
}

pub async fn put_presence(
    State(app): State<AppState>,
    Json(mut cfg): Json<PresenceConfig>,
) -> ApiResult<impl IntoResponse> {
    cfg.away_after_s = None;
    cfg.validate().map_err(bad_request)?;
    let accounts = &app.core().accounts;
    if let Some(p) = cfg
        .phones
        .iter()
        .find(|p| accounts.get(&p.user_id).is_none())
    {
        return Err(bad_request(format!(
            "le compte « {} » n'existe pas",
            p.user_id
        )));
    }
    app.core()
        .settings
        .set(presence::CONFIG_KEY, &cfg)
        .map_err(bad_request)?;
    Ok(Json(cfg))
}

pub async fn scan(State(app): State<AppState>) -> impl IntoResponse {
    Json(app.core().presence.scan().await)
}

// ---- Accès à distance --------------------------------------------------------------

/// Ce que Tailscale dit de cette machine (si Tailscale est installé ici).
fn tailscale_info() -> Value {
    let Ok(out) = std::process::Command::new("tailscale")
        .args(["status", "--json"])
        .output()
    else {
        return json!({ "installed": false });
    };
    let status: Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let dns = status
        .pointer("/Self/DNSName")
        .and_then(Value::as_str)
        .map(|s| s.trim_end_matches('.').to_string())
        .filter(|s| !s.is_empty());
    json!({
        "installed": true,
        "running": status.get("BackendState").and_then(Value::as_str) == Some("Running"),
        "dns_name": dns,
        "url": dns.as_ref().map(|d| format!("https://{d}")),
    })
}

pub async fn remote(State(app): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    let tailscale = tokio::task::spawn_blocking(tailscale_info)
        .await
        .unwrap_or_else(|_| json!({ "installed": false }));
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default()
        .to_string();
    Json(json!({
        "tailscale": tailscale,
        "secure": crate::api::is_secure(&headers),
        "host": host,
        "port": app.port(),
        "accounts": !app.core().accounts.is_open(),
    }))
}

// ---- Premier démarrage et réseau -------------------------------------------------

use axum::http::StatusCode;
use serde::Deserialize;

/// Public : l'assistant s'affiche-t-il ? Quel est l'état du réseau ?
pub async fn setup_status(
    State(app): State<AppState>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    let core = app.core();
    core.netsetup.touch();
    let mut network = core.netsetup.refresh().await;
    if let Some(e) = network.last_error.as_mut() {
        *e = crate::i18n::tr(crate::api::lang_of(&headers), e).into_owned();
    }
    // L'étape où reprendre (après le passage sur le Wi-Fi de la maison, par exemple).
    let resume = core.settings.get::<String>(SETUP_RESUME);
    Json(json!({
        // Pas encore de compte (obligatoire), assistant pas fini, ou retour sur le réseau
        // « Heim-XXXX » (Wi-Fi perdu, mot de passe faux).
        "needed": core.accounts.is_open() || resume.is_some() || network.mode == "hotspot",
        "accounts": !core.accounts.is_open(),
        "resume": resume,
        "hostname": hostname(),
        "timezone": timezone(),
        "netsetup": crate::netsetup::enabled(),
        "network": network,
        "version": env!("CARGO_PKG_VERSION"),
        "local_ip": presence::local_ipv4(),
    }))
}

pub async fn setup_scan(State(app): State<AppState>) -> impl IntoResponse {
    let core = app.core();
    core.netsetup.touch();
    Json(core.netsetup.scan().await)
}

pub const SETUP_RESUME: &str = "setup.resume";

fn hostname() -> Option<String> {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
}

fn timezone() -> Option<String> {
    std::fs::read_link("/etc/localtime").ok().and_then(|p| {
        let s = p.to_string_lossy().into_owned();
        s.split_once("zoneinfo/").map(|(_, tz)| tz.to_string())
    })
}

#[derive(Deserialize)]
pub struct WifiInput {
    ssid: String,
    #[serde(default)]
    password: String,
    /// Réseau caché (absent de la liste).
    #[serde(default)]
    hidden: bool,
    /// Adresse fixe (option avancée).
    #[serde(default, rename = "static")]
    fixed: Option<crate::netsetup::StaticIp>,
}

#[derive(Deserialize)]
pub struct StepInput {
    step: String,
}

/// L'assistant note où il en est : il reprendra là, même depuis une autre adresse.
pub async fn setup_step(
    State(app): State<AppState>,
    Json(input): Json<StepInput>,
) -> ApiResult<impl IntoResponse> {
    const STEPS: [&str; 7] = [
        "language",
        "account",
        "network",
        "look",
        "house",
        "connections",
        "summary",
    ];
    if !STEPS.contains(&input.step.as_str()) {
        return Err(bad_request("étape inconnue"));
    }
    app.core()
        .settings
        .set(SETUP_RESUME, &input.step)
        .map_err(bad_request)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Le câble est branché : on referme le Wi-Fi « Heim-XXXX ».
pub async fn setup_ethernet(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    if !crate::netsetup::enabled() {
        return Err(bad_request(
            "le réseau se règle sur la machine elle-même (Heim ne le gère pas ici)",
        ));
    }
    let net = app.core().netsetup.clone();
    let st = net.refresh().await;
    if !st.ethernet {
        return Err(bad_request("aucun câble réseau connecté"));
    }
    tokio::spawn(async move { net.use_ethernet().await });
    Ok(Json(
        json!({ "switching": true, "ip": st.ethernet_ip, "next": format!("http://{}.local", hostname().unwrap_or_else(|| "heim".into())) }),
    ))
}

async fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    let out = tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("{cmd} introuvable : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{cmd} : {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[derive(Deserialize)]
pub struct NameInput {
    name: String,
}

/// Le nom de la box sur le réseau (« heim » → http://heim.local).
pub async fn setup_hostname(Json(input): Json<NameInput>) -> ApiResult<impl IntoResponse> {
    if !crate::netsetup::enabled() {
        return Err(bad_request("le nom se règle sur la machine elle-même"));
    }
    let name = crate::netsetup::clean_hostname(&input.name)
        .ok_or_else(|| bad_request("nom invalide : lettres minuscules, chiffres et tirets"))?;
    run("hostnamectl", &["set-hostname", &name])
        .await
        .map_err(bad_request)?;
    Ok(Json(
        json!({ "hostname": name, "address": format!("http://{name}.local") }),
    ))
}

#[derive(Deserialize)]
pub struct TzInput {
    timezone: String,
}

/// Le fuseau horaire de la box (proposé d'après celui du téléphone).
pub async fn setup_timezone(Json(input): Json<TzInput>) -> ApiResult<impl IntoResponse> {
    if !crate::netsetup::enabled() {
        return Err(bad_request(
            "le fuseau horaire se règle sur la machine elle-même",
        ));
    }
    let tz = crate::netsetup::clean_timezone(&input.timezone)
        .ok_or_else(|| bad_request("fuseau horaire inconnu"))?;
    run("timedatectl", &["set-timezone", &tz])
        .await
        .map_err(bad_request)?;
    Ok(Json(json!({ "timezone": tz })))
}

/// Rejoint le Wi-Fi de la maison. Répond tout de suite : le téléphone va perdre la connexion
/// au réseau « Heim-XXXX » quelques secondes plus tard.
pub async fn setup_wifi(
    State(app): State<AppState>,
    Json(input): Json<WifiInput>,
) -> ApiResult<impl IntoResponse> {
    if !crate::netsetup::enabled() {
        return Err(bad_request(
            "le Wi-Fi se règle sur la machine elle-même (Heim ne le gère pas ici)",
        ));
    }
    let ssid = input.ssid.trim().to_string();
    if ssid.is_empty() {
        return Err(bad_request("choisissez un réseau Wi-Fi"));
    }
    // Un nom de Wi-Fi fait 32 octets au plus ; ni caractère de contrôle, ni tiret en tête
    // (il serait pris pour une option de nmcli).
    if ssid.len() > 32 || ssid.chars().any(char::is_control) || ssid.starts_with('-') {
        return Err(bad_request("nom de réseau Wi-Fi invalide"));
    }
    if input.password.len() > 63 || input.password.chars().any(char::is_control) {
        return Err(bad_request(
            "mot de passe Wi-Fi invalide (63 caractères au plus)",
        ));
    }
    if !input.password.is_empty() && input.password.len() < 8 {
        return Err(bad_request(
            "un mot de passe Wi-Fi fait au moins 8 caractères",
        ));
    }
    let fixed = input
        .fixed
        .as_ref()
        .filter(|f| !f.address.trim().is_empty())
        .map(|f| f.check())
        .transpose()
        .map_err(bad_request)?;
    let core = app.core().clone();
    let state = core.netsetup.state();
    let net = core.netsetup.clone();
    let password = input.password.clone();
    let target = ssid.clone();
    let hidden = input.hidden;
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let _ = net.connect(&target, &password, hidden, fixed).await;
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(
            json!({ "switching": true, "ssid": ssid, "hotspot_ssid": state.hotspot_ssid, "next": format!("http://{}.local", hostname().unwrap_or_else(|| "heim".into())) }),
        ),
    ))
}

pub async fn setup_done(State(app): State<AppState>) -> ApiResult<impl IntoResponse> {
    let core = app.core();
    if core.accounts.is_open() {
        return Err(bad_request("créez d'abord le compte administrateur"));
    }
    core.settings
        .set("setup.done", &true)
        .map_err(bad_request)?;
    core.settings.remove(SETUP_RESUME);
    Ok(StatusCode::NO_CONTENT)
}
